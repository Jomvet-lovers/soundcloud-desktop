import { Inject, Injectable, Logger, type OnModuleInit } from '@nestjs/common';
import { Cron } from '@nestjs/schedule';
import { QdrantClient } from '@qdrant/js-client-rest';
import { and, count, desc, eq, gte, inArray, isNotNull, sql } from 'drizzle-orm';
import { CentroidService } from '../centroids/centroid.service.js';
import { CollabVectorService } from '../collab/collab-vector.service.js';
import { userIdToQdrantId } from '../common/user-id.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { indexedTracks, userEvents } from '../db/schema.js';
import { LTR_FEATURE_COUNT, LtrService } from './ltr.service.js';

const TRAIN_WINDOW_DAYS = 30;
const MIN_POSITIVE_PER_USER = 10;
const MAX_USERS = 500;
const MAX_PAIRS_PER_USER = 80;
const MIN_NEGATIVES_PER_USER = 8;
const MIN_TOTAL_EXAMPLES = 500;

const LABELS: Record<string, number> = {
  like: 5,
  local_like: 5,
  playlist_add: 4,
  full_play: 3,
  skip: 0,
};

const POSITIVE_TYPES = ['like', 'local_like', 'playlist_add'];
const ALL_TYPES = Object.keys(LABELS);

interface TrackVectors {
  collab?: number[];
  mert?: number[];
  clap?: number[];
  lyrics?: number[];
  playbackCount: number;
  language: string | null;
}

@Injectable()
export class LtrTrainerService implements OnModuleInit {
  private readonly logger = new Logger(LtrTrainerService.name);
  private inProgress = false;

  constructor(
    @Inject('QDRANT_CLIENT') private readonly qdrant: QdrantClient,
    @Inject(DB) private readonly db: Database,
    private readonly collab: CollabVectorService,
    private readonly centroids: CentroidService,
    private readonly ltr: LtrService,
  ) {}

  async onModuleInit(): Promise<void> {
    setTimeout(
      () => {
        void this.trainNow().catch((e) =>
          this.logger.debug(`bootstrap ltr-train: ${(e as Error).message}`),
        );
      },
      5 * 60 * 1000,
    );
  }

  @Cron('0 4 * * 0')
  async scheduledTrain(): Promise<void> {
    if (process.env.LTR_AUTO_TRAIN === 'false') return;
    await this.trainNow().catch((e) =>
      this.logger.warn(`scheduled ltr-train failed: ${(e as Error).message}`),
    );
  }

  async trainNow(): Promise<{ enqueued: boolean; examples: number; reason?: string }> {
    if (this.inProgress) return { enqueued: false, examples: 0, reason: 'in_progress' };
    this.inProgress = true;
    try {
      const examples = await this.buildExamples();
      if (examples.length < MIN_TOTAL_EXAMPLES) {
        this.logger.warn(
          `[ltr.train] too few examples (${examples.length} < ${MIN_TOTAL_EXAMPLES}), skip`,
        );
        return { enqueued: false, examples: examples.length, reason: 'too_few_examples' };
      }
      this.logger.log(`[ltr.train] publishing ${examples.length} examples`);
      await this.ltr.publishTraining(examples);
      return { enqueued: true, examples: examples.length };
    } finally {
      this.inProgress = false;
    }
  }

  private async buildExamples(): Promise<
    Array<{ group: number; label: number; features: number[] }>
  > {
    const since = new Date(Date.now() - TRAIN_WINDOW_DAYS * 24 * 60 * 60 * 1000);

    const userCounts = await this.db
      .select({ scUserId: userEvents.scUserId, n: count() })
      .from(userEvents)
      .where(
        and(
          inArray(userEvents.eventType, POSITIVE_TYPES),
          gte(userEvents.createdAt, since),
        ),
      )
      .groupBy(userEvents.scUserId)
      .having(sql`COUNT(*) >= ${MIN_POSITIVE_PER_USER}`)
      .orderBy(desc(count()))
      .limit(MAX_USERS);

    if (!userCounts.length) {
      this.logger.warn('[ltr.train] no active users');
      return [];
    }

    const out: Array<{ group: number; label: number; features: number[] }> = [];
    let groupCounter = 0;

    for (const u of userCounts) {
      const examples = await this.buildUserExamples(u.scUserId, since, groupCounter);
      if (examples.length >= 2) {
        out.push(...examples);
        groupCounter++;
      }
    }
    return out;
  }

  private async buildUserExamples(
    scUserId: string,
    _since: Date,
    group: number,
  ): Promise<Array<{ group: number; label: number; features: number[] }>> {
    const events = await this.db
      .select({ scTrackId: userEvents.scTrackId, eventType: userEvents.eventType })
      .from(userEvents)
      .where(and(eq(userEvents.scUserId, scUserId), inArray(userEvents.eventType, ALL_TYPES)));

    const labelByTrack = new Map<string, number>();
    for (const e of events) {
      const lab = LABELS[e.eventType];
      if (lab === undefined) continue;
      const prev = labelByTrack.get(e.scTrackId);
      if (prev === undefined || lab > prev) labelByTrack.set(e.scTrackId, lab);
    }

    if (labelByTrack.size < 3) return [];

    const hasNegatives = [...labelByTrack.values()].some((v) => v === 0);
    if (!hasNegatives) {
      const random = await this.db
        .select({ scTrackId: indexedTracks.scTrackId })
        .from(indexedTracks)
        .where(isNotNull(indexedTracks.indexedAt))
        .orderBy(sql`RANDOM()`)
        .limit(MIN_NEGATIVES_PER_USER);
      for (const r of random) {
        if (!labelByTrack.has(r.scTrackId)) labelByTrack.set(r.scTrackId, 0);
      }
    }

    const userCollab = await this.collab.getUserVector(scUserId);
    const userTasteId = userIdToQdrantId(scUserId);
    const [userMert, userClap, userLyrics] = await Promise.all([
      this.retrieveSingle('user_taste_mert', userTasteId),
      this.retrieveSingle('user_taste_clap', userTasteId),
      this.retrieveSingle('user_taste_lyrics', userTasteId),
    ]);

    const trackIds = [...labelByTrack.keys()].slice(0, MAX_PAIRS_PER_USER);
    const numericIds = trackIds
      .map((id) => Number(id))
      .filter((n) => Number.isFinite(n)) as number[];
    if (!numericIds.length) return [];

    const trackVecs = await this.loadTrackVectors(numericIds);
    if (!trackVecs.size) return [];

    const userLangs = await this.detectUserLanguages(scUserId);

    const cMert = this.centroids.get('tracks_mert');
    const cClap = this.centroids.get('tracks_clap');

    const examples: Array<{ group: number; label: number; features: number[] }> = [];
    for (const id of trackIds) {
      const v = trackVecs.get(id);
      if (!v) continue;
      const label = labelByTrack.get(id) ?? 0;
      const feats = new Array<number>(LTR_FEATURE_COUNT).fill(0);
      feats[0] = userCollab && v.collab ? cosine(v.collab, userCollab) : 0;
      feats[1] = userMert && v.mert ? whitenedCos(v.mert, userMert, cMert) : 0;
      feats[2] = userClap && v.clap ? whitenedCos(v.clap, userClap, cClap) : 0;
      feats[3] = userLyrics && v.lyrics ? cosine(v.lyrics, userLyrics) : 0;
      feats[4] = Math.log1p(Math.max(0, v.playbackCount));
      feats[5] = v.language && userLangs.has(v.language) ? 1.0 : 0.0;
      examples.push({ group, label, features: feats });
    }
    return examples;
  }

  private async detectUserLanguages(scUserId: string): Promise<Set<string>> {
    const events = await this.db
      .select({ scTrackId: userEvents.scTrackId })
      .from(userEvents)
      .where(
        and(eq(userEvents.scUserId, scUserId), inArray(userEvents.eventType, POSITIVE_TYPES)),
      )
      .orderBy(desc(userEvents.createdAt))
      .limit(50);
    if (!events.length) return new Set();
    const ids = events.map((e) => e.scTrackId);
    const tracks = await this.db
      .select({ language: indexedTracks.language })
      .from(indexedTracks)
      .where(inArray(indexedTracks.scTrackId, ids));
    const counts = new Map<string, number>();
    for (const t of tracks) {
      const l = t.language;
      if (!l) continue;
      counts.set(l, (counts.get(l) ?? 0) + 1);
    }
    return new Set(
      [...counts.entries()]
        .sort((a, b) => b[1] - a[1])
        .slice(0, 3)
        .map(([l]) => l),
    );
  }

  private async loadTrackVectors(ids: number[]): Promise<Map<string, TrackVectors>> {
    const out = new Map<string, TrackVectors>();
    const [mertMap, clapMap, lyricsMap, collabMap, tracks] = await Promise.all([
      this.retrieveBatch('tracks_mert', ids),
      this.retrieveBatch('tracks_clap', ids),
      this.retrieveBatch('tracks_lyrics', ids),
      this.collab.getTrackVectors(ids),
      this.db
        .select({
          scTrackId: indexedTracks.scTrackId,
          rawScData: indexedTracks.rawScData,
          language: indexedTracks.language,
        })
        .from(indexedTracks)
        .where(inArray(indexedTracks.scTrackId, ids.map(String))),
    ]);
    const meta = new Map(tracks.map((t) => [t.scTrackId, t]));
    for (const id of ids) {
      const key = String(id);
      const t = meta.get(key);
      const raw = (t?.rawScData ?? {}) as { playback_count?: number };
      out.set(key, {
        collab: collabMap.get(key),
        mert: mertMap.get(key),
        clap: clapMap.get(key),
        lyrics: lyricsMap.get(key),
        playbackCount: Number(raw.playback_count ?? 0),
        language: t?.language ?? null,
      });
    }
    return out;
  }

  private async retrieveSingle(coll: string, id: number): Promise<number[] | null> {
    try {
      const pts = (await this.qdrant.retrieve(coll, {
        ids: [id],
        with_vector: true,
        with_payload: false,
      })) as unknown as Array<{ vector?: number[] | null }>;
      const v = pts[0]?.vector;
      return Array.isArray(v) ? v : null;
    } catch {
      return null;
    }
  }

  private async retrieveBatch(coll: string, ids: number[]): Promise<Map<string, number[]>> {
    const out = new Map<string, number[]>();
    if (!ids.length) return out;
    try {
      const pts = (await this.qdrant.retrieve(coll, {
        ids,
        with_vector: true,
        with_payload: false,
      })) as unknown as Array<{ id: string | number; vector?: number[] | null }>;
      for (const p of pts) {
        if (Array.isArray(p.vector)) out.set(String(p.id), p.vector);
      }
    } catch (e) {
      this.logger.debug(`retrieveBatch ${coll}: ${(e as Error).message}`);
    }
    return out;
  }
}

function cosine(a: number[], b: number[]): number {
  const n = Math.min(a.length, b.length);
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let i = 0; i < n; i++) {
    dot += a[i] * b[i];
    na += a[i] * a[i];
    nb += b[i] * b[i];
  }
  const denom = Math.sqrt(na) * Math.sqrt(nb);
  return denom > 0 ? dot / denom : 0;
}

function whitenedCos(a: number[], b: number[], centroid: number[] | null): number {
  if (!centroid) return cosine(a, b);
  const n = Math.min(a.length, b.length, centroid.length);
  const aw = new Array<number>(n);
  const bw = new Array<number>(n);
  for (let i = 0; i < n; i++) {
    aw[i] = a[i] - centroid[i];
    bw[i] = b[i] - centroid[i];
  }
  return cosine(aw, bw);
}
