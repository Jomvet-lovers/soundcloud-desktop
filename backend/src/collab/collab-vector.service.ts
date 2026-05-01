import { Inject, Injectable, Logger, type OnModuleInit } from '@nestjs/common';
import { QdrantClient } from '@qdrant/js-client-rest';
import { and, desc, eq, inArray } from 'drizzle-orm';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { userEvents } from '../db/schema.js';

const TTL_MS = 5 * 60 * 1000;
const MAX_LIKES = 50;

const POSITIVE_TYPES = ['like', 'local_like', 'playlist_add'];

interface CacheEntry {
  vector: number[] | null;
  expiresAt: number;
}

@Injectable()
export class CollabVectorService implements OnModuleInit {
  private readonly logger = new Logger(CollabVectorService.name);
  private readonly cache = new Map<string, CacheEntry>();
  private collabDim: number | null = null;
  private collabDimCheckedAt = 0;

  constructor(
    @Inject('QDRANT_CLIENT') private readonly qdrant: QdrantClient,
    @Inject(DB) private readonly db: Database,
  ) {}

  async onModuleInit(): Promise<void> {
    await this.detectCollabDim().catch(() => {});
  }

  async getCollabDim(): Promise<number | null> {
    if (this.collabDim !== null && Date.now() - this.collabDimCheckedAt < 60_000) {
      return this.collabDim;
    }
    return this.detectCollabDim();
  }

  private async detectCollabDim(): Promise<number | null> {
    try {
      const info = (await this.qdrant.getCollection('tracks_collab')) as {
        config?: { params?: { vectors?: { size?: number } } };
      };
      this.collabDim = info.config?.params?.vectors?.size ?? null;
    } catch {
      this.collabDim = null;
    }
    this.collabDimCheckedAt = Date.now();
    return this.collabDim;
  }

  async getTrackVector(scTrackId: number): Promise<number[] | null> {
    try {
      const pts = (await this.qdrant.retrieve('tracks_collab', {
        ids: [scTrackId],
        with_vector: true,
        with_payload: false,
      })) as unknown as Array<{ vector?: number[] | null }>;
      const v = pts[0]?.vector;
      return Array.isArray(v) ? v : null;
    } catch {
      return null;
    }
  }

  async getTrackVectors(ids: number[]): Promise<Map<string, number[]>> {
    const out = new Map<string, number[]>();
    if (!ids.length) return out;
    try {
      const pts = (await this.qdrant.retrieve('tracks_collab', {
        ids,
        with_vector: true,
        with_payload: false,
      })) as unknown as Array<{ id: string | number; vector?: number[] | null }>;
      for (const p of pts) {
        if (Array.isArray(p.vector)) out.set(String(p.id), p.vector);
      }
    } catch (e) {
      this.logger.debug(`getTrackVectors failed: ${(e as Error).message}`);
    }
    return out;
  }

  async getUserVector(scUserId: string): Promise<number[] | null> {
    const cached = this.cache.get(scUserId);
    if (cached && cached.expiresAt > Date.now()) return cached.vector;

    const events = await this.db
      .select({ scTrackId: userEvents.scTrackId })
      .from(userEvents)
      .where(
        and(
          eq(userEvents.scUserId, scUserId),
          inArray(userEvents.eventType, POSITIVE_TYPES),
        ),
      )
      .orderBy(desc(userEvents.createdAt))
      .limit(MAX_LIKES);

    if (!events.length) {
      this.cache.set(scUserId, { vector: null, expiresAt: Date.now() + TTL_MS });
      return null;
    }
    const ids = events
      .map((e) => Number(e.scTrackId))
      .filter((n) => Number.isFinite(n)) as number[];
    const vecs = await this.getTrackVectors(ids);
    if (vecs.size === 0) {
      this.cache.set(scUserId, { vector: null, expiresAt: Date.now() + TTL_MS });
      return null;
    }

    const dim = [...vecs.values()][0].length;
    const acc = new Array<number>(dim).fill(0);
    for (const v of vecs.values()) {
      for (let i = 0; i < dim; i++) acc[i] += v[i];
    }
    for (let i = 0; i < dim; i++) acc[i] /= vecs.size;
    const norm = Math.sqrt(acc.reduce((s, v) => s + v * v, 0));
    const normalized = norm > 0 ? acc.map((v) => v / norm) : acc;

    this.cache.set(scUserId, { vector: normalized, expiresAt: Date.now() + TTL_MS });
    return normalized;
  }

  invalidate(scUserId: string): void {
    this.cache.delete(scUserId);
  }

  invalidateAll(): void {
    this.cache.clear();
    this.collabDim = null;
  }
}
