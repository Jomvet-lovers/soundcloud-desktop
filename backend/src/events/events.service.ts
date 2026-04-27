import { forwardRef, Inject, Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { In, IsNull, Repository } from 'typeorm';
import { normalizeScTrackId } from '../common/sc-ids.js';
import { DislikesService } from '../dislikes/dislikes.service.js';
import { IndexingService } from '../indexing/indexing.service.js';
import { UserTasteService } from '../user-taste/user-taste.service.js';
import { UserEvent } from './entities/user-event.entity.js';

const EVENT_WEIGHTS: Record<string, number> = {
  like: 1.0,
  local_like: 1.0,
  playlist_add: 0.9,
  full_play: 0.3,
  skip: -0.5,
  dislike: -1.0,
};

const POSITIVE_EVENTS = new Set(['like', 'local_like', 'playlist_add', 'full_play']);

@Injectable()
export class EventsService {
  private readonly logger = new Logger(EventsService.name);
  private readonly userLocks = new Map<string, Promise<void>>();

  constructor(
    @InjectRepository(UserEvent)
    private readonly repo: Repository<UserEvent>,
    private readonly userTaste: UserTasteService,
    private readonly indexing: IndexingService,
    @Inject(forwardRef(() => DislikesService))
    private readonly dislikes: DislikesService,
  ) {}

  private async runSerially(key: string, fn: () => Promise<void>): Promise<void> {
    const prev = this.userLocks.get(key) ?? Promise.resolve();
    let release!: () => void;
    const current = new Promise<void>((r) => {
      release = r;
    });
    const tail = prev.then(() => current);
    this.userLocks.set(key, tail);
    await prev;
    try {
      await fn();
    } finally {
      release();
      if (this.userLocks.get(key) === tail) this.userLocks.delete(key);
    }
  }

  /**
   * Try to apply a single event to the taste vector with current-state guards.
   * - Returns true if event was handled (either EMA applied or deliberately skipped by guard).
   * - Returns false if track isn't indexed yet — caller should enqueue it.
   */
  private async tryApply(event: UserEvent): Promise<boolean> {
    const isCurrentlyDisliked = await this.dislikes.isDislikedByUserId(
      event.scUserId,
      event.scTrackId,
    );

    if (POSITIVE_EVENTS.has(event.eventType) && isCurrentlyDisliked) {
      await this.repo.update(event.id, { tasteAppliedAt: new Date() });
      return true;
    }

    if (event.eventType === 'dislike' && !isCurrentlyDisliked) {
      await this.repo.update(event.id, { tasteAppliedAt: new Date() });
      return true;
    }

    const applied = await this.userTaste.onUserEvent(
      event.scUserId,
      event.scTrackId,
      event.eventType,
    );
    if (applied) {
      await this.repo.update(event.id, { tasteAppliedAt: new Date() });
      return true;
    }
    return false;
  }

  async record(scUserId: string, scTrackId: string, eventType: string): Promise<void> {
    const weight = EVENT_WEIGHTS[eventType];
    if (weight === undefined) {
      this.logger.warn(`Unknown event type: ${eventType}`);
      return;
    }
    const normalizedId = normalizeScTrackId(scTrackId);
    if (!normalizedId) {
      this.logger.warn(`Invalid scTrackId: ${scTrackId}`);
      return;
    }

    await this.runSerially(`events:${scUserId}`, async () => {
      const event = await this.repo.save(
        this.repo.create({
          scUserId,
          scTrackId: normalizedId,
          eventType,
          weight,
          seeded: false,
        }),
      );
      const applied = await this.tryApply(event);
      if (!applied) {
        this.indexing.ensureTrackQueuedById(normalizedId).catch((e) => {
          this.logger.error(`Failed to enqueue ${normalizedId}: ${(e as Error).message}`);
        });
      }
    });
  }

  /**
   * Backfill missing `like` events for tracks the user has already liked on SoundCloud.
   * Idempotent across calls: only records events for tracks without a `like` yet.
   * For unindexed tracks, enqueues them — consumer will apply taste when worker finishes.
   */
  async ensureLikesRecorded(scUserId: string, scTrackIds: string[]): Promise<void> {
    if (!scUserId || scTrackIds.length === 0) return;

    const normalized = scTrackIds
      .map((id) => normalizeScTrackId(id))
      .filter((id): id is string => !!id);
    if (normalized.length === 0) return;

    await this.runSerially(`events:${scUserId}`, async () => {
      const existing = await this.repo.find({
        select: { scTrackId: true },
        where: { scUserId, eventType: 'like', scTrackId: In(normalized) },
      });
      const existingSet = new Set(existing.map((e) => e.scTrackId));
      const missing = [...new Set(normalized.filter((id) => !existingSet.has(id)))];
      if (missing.length === 0) return;

      const weight = EVENT_WEIGHTS.like;
      const saved = await this.repo.save(
        missing.map((scTrackId) =>
          this.repo.create({ scUserId, scTrackId, eventType: 'like', weight, seeded: true }),
        ),
      );

      for (const event of saved) {
        const applied = await this.tryApply(event);
        if (!applied) {
          this.indexing.ensureTrackQueuedById(event.scTrackId).catch((e) => {
            this.logger.error(`Failed to enqueue ${event.scTrackId}: ${(e as Error).message}`);
          });
        }
      }
    });
  }

  /**
   * Called by IndexingQueueConsumer when worker finishes indexing a track.
   * Applies all pending events for this track across users, serialized per-user.
   */
  async applyPendingEventsForTrack(scTrackId: string): Promise<void> {
    const pending = await this.repo.find({
      where: { scTrackId, tasteAppliedAt: IsNull() },
      order: { createdAt: 'ASC' },
    });
    if (pending.length === 0) return;

    const byUser = new Map<string, UserEvent[]>();
    for (const e of pending) {
      const list = byUser.get(e.scUserId) ?? [];
      list.push(e);
      byUser.set(e.scUserId, list);
    }

    await Promise.all(
      Array.from(byUser.entries()).map(([userId, events]) =>
        this.runSerially(`events:${userId}`, async () => {
          for (const event of events) {
            try {
              await this.tryApply(event);
            } catch (e) {
              this.logger.error(`tryApply failed for event ${event.id}: ${(e as Error).message}`);
            }
          }
        }),
      ),
    );
  }

  async getRecentLiked(scUserId: string, limit = 5): Promise<string[]> {
    const events = await this.repo.find({
      where: { scUserId, eventType: 'like' },
      order: { createdAt: 'DESC' },
      take: limit,
      select: ['scTrackId'],
    });
    return events.map((e) => e.scTrackId);
  }

  async getRecentSkipped(scUserId: string, limit = 3): Promise<string[]> {
    const events = await this.repo.find({
      where: { scUserId, eventType: 'skip' },
      order: { createdAt: 'DESC' },
      take: limit,
      select: ['scTrackId'],
    });
    return events.map((e) => e.scTrackId);
  }

  async getRecentPlayed(scUserId: string, limit = 50): Promise<string[]> {
    const events = await this.repo.find({
      where: { scUserId },
      order: { createdAt: 'DESC' },
      take: limit,
      select: ['scTrackId'],
    });
    return [...new Set(events.map((e) => e.scTrackId))];
  }
}
