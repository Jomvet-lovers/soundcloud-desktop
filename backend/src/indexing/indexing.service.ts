import { Inject, Injectable, Logger, OnModuleDestroy, OnModuleInit } from '@nestjs/common';
import { and, count, eq, isNotNull, isNull, lt, sql } from 'drizzle-orm';
import { NatsService } from '../bus/nats.service.js';
import { STREAMS, SUBJECTS } from '../bus/subjects.js';
import { normalizeScTrackId } from '../common/sc-ids.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { indexedTracks } from '../db/schema.js';
import { LyricsService } from '../lyrics/lyrics.service.js';
import type { ScTrack } from '../soundcloud/soundcloud.types.js';
import { TranscodeTriggerService } from '../transcode/transcode-trigger.service.js';

const REAP_INTERVAL_MS = 5 * 60 * 1000;
const REAP_AGE_MS = 5 * 60 * 1000;
const REAP_BATCH = 50;

@Injectable()
export class IndexingService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(IndexingService.name);
  private reapTimer?: NodeJS.Timeout;

  constructor(
    @Inject(DB) private readonly db: Database,
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
    const existing = await this.db.query.indexedTracks.findFirst({
      where: eq(indexedTracks.scTrackId, scTrackId),
      columns: { id: true, indexedAt: true },
    });
    if (existing?.indexedAt) return;

    if (!existing) {
      const inserted = await this.db
        .insert(indexedTracks)
        .values({
          scTrackId,
          title: scTrack.title,
          genre: scTrack.genre ?? null,
          tags: scTrack.tag_list?.split(' ').filter(Boolean) ?? [],
          durationMs: scTrack.duration,
          artworkUrl: scTrack.artwork_url ?? null,
          streamUrl: scTrack.stream_url ?? null,
          rawScData: scTrack as unknown as Record<string, unknown>,
          indexedAt: null,
        })
        .onConflictDoNothing({ target: indexedTracks.scTrackId })
        .returning({ id: indexedTracks.id });
      if (inserted.length === 0) return;
    }

    this.trigger.trigger(scTrackId);
    this.lyrics.ensureLyricsForIndexing(scTrackId).catch(() => {});
  }

  async ensureTracksIndexed(tracks: ScTrack[]): Promise<void> {
    for (const track of tracks) {
      this.ensureTrackIndexed(track).catch(() => {});
    }
  }

  async ensureTrackQueuedById(scTrackId: string): Promise<void> {
    const track = await this.db.query.indexedTracks.findFirst({
      where: eq(indexedTracks.scTrackId, scTrackId),
      columns: { scTrackId: true, indexedAt: true },
    });
    if (!track) {
      this.logger.warn(`Cannot queue ${scTrackId}: not in indexed_tracks`);
      return;
    }
    if (track.indexedAt) return;
    this.trigger.trigger(scTrackId);
  }

  async getStats(): Promise<{ indexed: number; pending: number }> {
    const totalRows = await this.db.select({ n: count() }).from(indexedTracks);
    const indexedRows = await this.db
      .select({ n: count() })
      .from(indexedTracks)
      .where(isNotNull(indexedTracks.indexedAt));
    const total = totalRows[0]?.n ?? 0;
    const indexedCount = indexedRows[0]?.n ?? 0;
    return { indexed: indexedCount, pending: total - indexedCount };
  }

  private async subscribeStorageUploaded(): Promise<void> {
    await this.nats.consume(
      STREAMS.storageEvents.name,
      'backend-storage-uploaded',
      async (data) => {
        const payload = data as { sc_track_id?: string; storage_url?: string };
        const scTrackId = normalizeScTrackId(payload.sc_track_id);
        if (!scTrackId || !payload.storage_url) return;

        const existing = await this.db.query.indexedTracks.findFirst({
          where: eq(indexedTracks.scTrackId, scTrackId),
          columns: { id: true, indexedAt: true },
        });
        if (existing?.indexedAt) {
          await this.db
            .update(indexedTracks)
            .set({ s3VerifiedAt: sql`now()`, s3MissingAt: null })
            .where(eq(indexedTracks.scTrackId, scTrackId));
          return;
        }

        if (!existing) {
          const inserted = await this.db
            .insert(indexedTracks)
            .values({
              scTrackId,
              indexedAt: null,
              s3VerifiedAt: sql`now()` as unknown as Date,
              s3MissingAt: null,
            })
            .onConflictDoNothing({ target: indexedTracks.scTrackId })
            .returning({ id: indexedTracks.id });
          if (inserted.length === 0) {
            const post = await this.db.query.indexedTracks.findFirst({
              where: eq(indexedTracks.scTrackId, scTrackId),
              columns: { indexedAt: true },
            });
            if (post?.indexedAt) {
              await this.db
                .update(indexedTracks)
                .set({ s3VerifiedAt: sql`now()`, s3MissingAt: null })
                .where(eq(indexedTracks.scTrackId, scTrackId));
              return;
            }
          }
        } else {
          await this.db
            .update(indexedTracks)
            .set({ s3VerifiedAt: sql`now()`, s3MissingAt: null })
            .where(eq(indexedTracks.scTrackId, scTrackId));
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
        await this.db
          .update(indexedTracks)
          .set({ indexedAt: sql`now()` })
          .where(
            and(
              eq(indexedTracks.scTrackId, payload.sc_track_id),
              isNull(indexedTracks.indexedAt),
            ),
          );
        this.logger.debug(`indexed_at set for ${payload.sc_track_id}`);
      },
      SUBJECTS.doneIndexAudio,
    );
  }

  private async reap(): Promise<void> {
    const cutoff = new Date(Date.now() - REAP_AGE_MS);
    const stuck = await this.db
      .select({ scTrackId: indexedTracks.scTrackId })
      .from(indexedTracks)
      .where(and(isNull(indexedTracks.indexedAt), lt(indexedTracks.createdAt, cutoff)))
      .limit(REAP_BATCH);
    if (!stuck.length) return;
    this.logger.log(`reaping ${stuck.length} stuck tracks`);
    for (const t of stuck) {
      this.trigger.trigger(t.scTrackId);
    }
  }
}
