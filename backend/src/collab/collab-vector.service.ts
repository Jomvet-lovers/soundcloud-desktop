import { Inject, Injectable, Logger, type OnModuleInit } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { QdrantClient } from '@qdrant/js-client-rest';
import { In, Repository } from 'typeorm';
import { UserEvent } from '../events/entities/user-event.entity.js';

/**
 * Collab-вектор юзера = mean(last N positive event vectors) в пространстве item2vec.
 * В отличие от EMA на аудио-эмбеддингах, это пространство изотропно — простое
 * усреднение даёт хороший «центр вкуса» юзера. TTL-кеш в RAM, инвалидируется
 * при /collab/refresh-user или сам через TTL.
 */
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
    @Inject('QDRANT_CLIENT')
    private readonly qdrant: QdrantClient,
    @InjectRepository(UserEvent)
    private readonly events: Repository<UserEvent>,
  ) {}

  async onModuleInit(): Promise<void> {
    await this.detectCollabDim().catch(() => {});
  }

  /** Текущая размерность collab-коллекции (или null если коллекции нет). */
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

  /**
   * Для трека (sc_track_id) — собрать вектор из tracks_collab.
   * null если трек не в vocab item2vec (новый/редкий).
   */
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

  /**
   * Достать collab-вектора для пачки треков. Возвращает Map(id → vec).
   * Треки которых нет в vocab — отсутствуют в Map.
   */
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

  /**
   * Юзер-вектор в collab-пространстве: mean(last N positive likes).
   * Кешируется на TTL_MS, инвалидируется ручкой `invalidate()`.
   * Возвращает null если у юзера нет лайков с известными векторами в vocab.
   */
  async getUserVector(scUserId: string): Promise<number[] | null> {
    const cached = this.cache.get(scUserId);
    if (cached && cached.expiresAt > Date.now()) return cached.vector;

    const events = await this.events.find({
      where: {
        scUserId,
        eventType: In(POSITIVE_TYPES),
      },
      order: { createdAt: 'DESC' },
      take: MAX_LIKES,
      select: ['scTrackId'],
    });
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

  /** Сбросить кеш юзера (вызывать при новом like-событии для немедленного эффекта). */
  invalidate(scUserId: string): void {
    this.cache.delete(scUserId);
  }

  /** Полный сброс (после рестарта item2vec — все вектора могут поменяться). */
  invalidateAll(): void {
    this.cache.clear();
    this.collabDim = null;
  }
}
