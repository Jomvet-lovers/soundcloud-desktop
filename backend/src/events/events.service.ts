import { forwardRef, Inject, Injectable, Logger } from '@nestjs/common';
import { and, asc, desc, eq, inArray, isNull } from 'drizzle-orm';
import { CollabTrainerService } from '../collab/collab-trainer.service.js';
import { CollabVectorService } from '../collab/collab-vector.service.js';
import { normalizeScTrackId } from '../common/sc-ids.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { type UserEvent, userEvents } from '../db/schema.js';
import { DislikesService } from '../dislikes/dislikes.service.js';
import { IndexingService } from '../indexing/indexing.service.js';
import { UserTasteService } from '../user-taste/user-taste.service.js';

const EVENT_WEIGHTS: Record<string, number> = {
  like: 1.0,
  local_like: 1.0,
  playlist_add: 0.9,
  full_play: 0.3,
  skip: -0.5,
  dislike: -1.0,
};

const POSITIVE_EVENTS = new Set(['like', 'local_like', 'playlist_add']);
const TASTE_EVENTS = POSITIVE_EVENTS;
const COLLAB_TRIGGER_EVENTS = new Set(['like', 'local_like', 'playlist_add', 'full_play', 'skip']);

@Injectable()
export class EventsService {
  private readonly logger = new Logger(EventsService.name);
  private readonly userLocks = new Map<string, Promise<void>>();

  constructor(
    @Inject(DB) private readonly db: Database,
    private readonly userTaste: UserTasteService,
    private readonly indexing: IndexingService,
    @Inject(forwardRef(() => DislikesService))
    private readonly dislikes: DislikesService,
    private readonly collab: CollabVectorService,
    private readonly collabTrainer: CollabTrainerService,
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

  private async markApplied(id: string): Promise<void> {
    await this.db
      .update(userEvents)
      .set({ tasteAppliedAt: new Date() })
      .where(eq(userEvents.id, id));
  }

  private async tryApply(event: UserEvent): Promise<boolean> {
    const isCurrentlyDisliked = await this.dislikes.isDislikedByUserId(
      event.scUserId,
      event.scTrackId,
    );

    if (POSITIVE_EVENTS.has(event.eventType) && isCurrentlyDisliked) {
      await this.markApplied(event.id);
      return true;
    }

    if (!TASTE_EVENTS.has(event.eventType)) {
      await this.markApplied(event.id);
      return true;
    }

    const applied = await this.userTaste.onUserEvent(
      event.scUserId,
      event.scTrackId,
      event.eventType,
    );
    if (applied) {
      await this.markApplied(event.id);
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
      const [event] = await this.db
        .insert(userEvents)
        .values({ scUserId, scTrackId: normalizedId, eventType, weight, seeded: false })
        .returning();
      const applied = await this.tryApply(event);
      if (!applied) {
        this.indexing.ensureTrackQueuedById(normalizedId).catch((e) => {
          this.logger.error(`Failed to enqueue ${normalizedId}: ${(e as Error).message}`);
        });
      }
      if (POSITIVE_EVENTS.has(eventType)) this.collab.invalidate(scUserId);
      if (COLLAB_TRIGGER_EVENTS.has(eventType)) this.collabTrainer.noteEvent();
    });
  }

  async ensureLikesRecorded(scUserId: string, scTrackIds: string[]): Promise<void> {
    if (!scUserId || scTrackIds.length === 0) return;

    const normalized = scTrackIds
      .map((id) => normalizeScTrackId(id))
      .filter((id): id is string => !!id);
    if (normalized.length === 0) return;

    await this.runSerially(`events:${scUserId}`, async () => {
      const existing = await this.db
        .select({ scTrackId: userEvents.scTrackId })
        .from(userEvents)
        .where(
          and(
            eq(userEvents.scUserId, scUserId),
            eq(userEvents.eventType, 'like'),
            inArray(userEvents.scTrackId, normalized),
          ),
        );
      const existingSet = new Set(existing.map((e) => e.scTrackId));
      const missing = [...new Set(normalized.filter((id) => !existingSet.has(id)))];
      if (missing.length === 0) return;

      const weight = EVENT_WEIGHTS.like;
      const saved = await this.db
        .insert(userEvents)
        .values(
          missing.map((scTrackId) => ({
            scUserId,
            scTrackId,
            eventType: 'like',
            weight,
            seeded: true,
          })),
        )
        .returning();

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

  async applyPendingEventsForTrack(scTrackId: string): Promise<void> {
    const pending = await this.db
      .select()
      .from(userEvents)
      .where(and(eq(userEvents.scTrackId, scTrackId), isNull(userEvents.tasteAppliedAt)))
      .orderBy(asc(userEvents.createdAt));
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
    const rows = await this.db
      .select({ scTrackId: userEvents.scTrackId })
      .from(userEvents)
      .where(and(eq(userEvents.scUserId, scUserId), eq(userEvents.eventType, 'like')))
      .orderBy(desc(userEvents.createdAt))
      .limit(limit);
    return rows.map((e) => e.scTrackId);
  }

  async getRecentSkipped(scUserId: string, limit = 3): Promise<string[]> {
    const rows = await this.db
      .select({ scTrackId: userEvents.scTrackId })
      .from(userEvents)
      .where(and(eq(userEvents.scUserId, scUserId), eq(userEvents.eventType, 'skip')))
      .orderBy(desc(userEvents.createdAt))
      .limit(limit);
    return rows.map((e) => e.scTrackId);
  }

  async getRecentPlayed(scUserId: string, limit = 50): Promise<string[]> {
    const rows = await this.db
      .select({ scTrackId: userEvents.scTrackId })
      .from(userEvents)
      .where(eq(userEvents.scUserId, scUserId))
      .orderBy(desc(userEvents.createdAt))
      .limit(limit);
    return [...new Set(rows.map((e) => e.scTrackId))];
  }
}
