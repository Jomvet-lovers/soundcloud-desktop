import { Injectable, Logger, OnModuleDestroy, OnModuleInit } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, LessThan, Repository } from 'typeorm';
import { NatsService } from '../bus/nats.service.js';
import { STREAMS, SUBJECTS } from '../bus/subjects.js';
import { normalizeScTrackId } from '../common/sc-ids.js';
import { LyricsService } from '../lyrics/lyrics.service.js';
import type { ScTrack } from '../soundcloud/soundcloud.types.js';
import { TranscodeTriggerService } from '../transcode/transcode-trigger.service.js';
import { IndexedTrack } from './entities/indexed-track.entity.js';

const REAP_INTERVAL_MS = 5 * 60 * 1000;
const REAP_AGE_MS = 5 * 60 * 1000;
const REAP_BATCH = 50;

@Injectable()
export class IndexingService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(IndexingService.name);
  private reapTimer?: NodeJS.Timeout;

  constructor(
    @InjectRepository(IndexedTrack)
    private readonly repo: Repository<IndexedTrack>,
    private readonly nats: NatsService,
    private readonly lyrics: LyricsService,
    private readonly trigger: TranscodeTriggerService,
  ) {}

  async onModuleInit(): Promise<void> {
    await this.subscribeDone();
    await this.subscribeStorageUploaded();
    this.reapTimer = setInterval(() => this.reap().catch(() => {}), REAP_INTERVAL_MS);
  }

  onModuleDestroy(): void {
    if (this.reapTimer) clearInterval(this.reapTimer);
  }

  async ensureTrackIndexed(scTrack: ScTrack): Promise<void> {
    const scTrackId = String(scTrack.urn?.split(':').pop() ?? scTrack.urn);
    const existing = await this.repo.findOne({ where: { scTrackId }, select: ['id', 'indexedAt'] });
    if (existing?.indexedAt) return;

    if (!existing) {
      const inserted = await this.repo
        .createQueryBuilder()
        .insert()
        .into(IndexedTrack)
        .values({
          scTrackId,
          title: scTrack.title,
          genre: scTrack.genre ?? null,
          tags: scTrack.tag_list?.split(' ').filter(Boolean) ?? [],
          durationMs: scTrack.duration,
          artworkUrl: scTrack.artwork_url ?? null,
          streamUrl: scTrack.stream_url ?? null,
          rawScData: scTrack as any,
          indexedAt: null,
        })
        .orIgnore()
        .execute();
      if (!inserted.identifiers.length) return;
    }

    this.trigger.trigger(scTrackId);
    this.lyrics.ensureLyricsForIndexing(scTrackId).catch(() => {});
  }

  async ensureTracksIndexed(tracks: ScTrack[]): Promise<void> {
    for (const track of tracks) {
      this.ensureTrackIndexed(track).catch(() => {});
    }
  }

  /** Внешний ре-ентер (ручной админский). */
  async ensureTrackQueuedById(scTrackId: string): Promise<void> {
    const track = await this.repo.findOne({
      where: { scTrackId },
      select: ['scTrackId', 'indexedAt'],
    });
    if (!track) {
      this.logger.warn(`Cannot queue ${scTrackId}: not in indexed_tracks`);
      return;
    }
    if (track.indexedAt) return;
    this.trigger.trigger(scTrackId);
  }

  async getStats(): Promise<{ indexed: number; pending: number }> {
    const total = await this.repo.count();
    const indexedCount = await this.repo
      .createQueryBuilder('t')
      .where('t.indexed_at IS NOT NULL')
      .getCount();
    return { indexed: indexedCount, pending: total - indexedCount };
  }

  // ──────────────────────────────────────────────────────────────
  // pipeline
  // ──────────────────────────────────────────────────────────────

  private async subscribeStorageUploaded(): Promise<void> {
    await this.nats.consume(
      STREAMS.storageEvents.name,
      'backend-storage-uploaded',
      async (data) => {
        const payload = data as { sc_track_id?: string; storage_url?: string };
        const scTrackId = normalizeScTrackId(payload.sc_track_id);
        if (!scTrackId || !payload.storage_url) return;

        const existing = await this.repo.findOne({
          where: { scTrackId },
          select: ['id', 'indexedAt'],
        });
        if (existing?.indexedAt) {
          await this.repo.update({ scTrackId }, { s3VerifiedAt: () => 'now()', s3MissingAt: null });
          return;
        }

        if (!existing) {
          const inserted = await this.repo
            .createQueryBuilder()
            .insert()
            .into(IndexedTrack)
            .values({
              scTrackId,
              indexedAt: null,
              s3VerifiedAt: () => 'now()',
              s3MissingAt: null,
            })
            .orIgnore()
            .execute();
          if (!inserted.identifiers.length) {
            const post = await this.repo.findOne({
              where: { scTrackId },
              select: ['indexedAt'],
            });
            if (post?.indexedAt) {
              await this.repo.update(
                { scTrackId },
                { s3VerifiedAt: () => 'now()', s3MissingAt: null },
              );
              return;
            }
          }
        } else {
          await this.repo.update({ scTrackId }, { s3VerifiedAt: () => 'now()', s3MissingAt: null });
        }

        await this.nats.publish(SUBJECTS.indexAudio, {
          sc_track_id: scTrackId,
          s3_url: payload.storage_url,
        });
        this.logger.log(`[storage→index] ${scTrackId} → NATS`);
        this.lyrics
          .handleUploaded(scTrackId, payload.storage_url)
          .catch((e) =>
            this.logger.debug(`lyrics handleUploaded ${scTrackId}: ${(e as Error).message}`),
          );
      },
      SUBJECTS.storageTrackUploaded,
    );
  }

  private async subscribeDone(): Promise<void> {
    await this.nats.consume(
      STREAMS.done.name,
      'backend-done-index-audio',
      async (data) => {
        const payload = data as { sc_track_id?: string };
        if (!payload.sc_track_id) return;
        await this.repo.update(
          { scTrackId: payload.sc_track_id, indexedAt: IsNull() },
          { indexedAt: () => 'now()' as any },
        );
        this.logger.debug(`indexed_at set for ${payload.sc_track_id}`);
      },
      SUBJECTS.doneIndexAudio,
    );
  }

  /** Крон: переотправляет зависшие (INSERT прошёл, pipeline упал). */
  private async reap(): Promise<void> {
    const cutoff = new Date(Date.now() - REAP_AGE_MS);
    const stuck = await this.repo.find({
      where: { indexedAt: IsNull(), createdAt: LessThan(cutoff) },
      select: ['scTrackId'],
      take: REAP_BATCH,
    });
    if (!stuck.length) return;
    this.logger.log(`reaping ${stuck.length} stuck tracks`);
    for (const t of stuck) {
      this.trigger.trigger(t.scTrackId);
    }
  }
}
