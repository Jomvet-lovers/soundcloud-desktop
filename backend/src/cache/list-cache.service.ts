import { Inject, Injectable, Logger } from '@nestjs/common';
import type { Redis } from 'ioredis';
import { REDIS_CLIENT } from './cache.constants.js';

type CacheScope = 'shared' | 'user';

interface ListState<T> {
  items: T[];
  next_cursor?: string;
  exhausted: boolean;
}

const LIST_PREFIX = 'list:';
const DEFAULT_CHUNK_SIZE = 200;
const MAX_CHUNKS_PER_REQUEST = 8;

export interface ListPageResult<T> {
  collection: T[];
  page: number;
  page_size: number;
  has_more: boolean;
}

export interface FetchChunkResult<T> {
  items: T[];
  nextCursor?: string;
}

export interface GetPageOptions<T> {
  /** Логический ключ ресурса: `related:{trackUrn}`, `me-likes:{sessionId}` и т.п. */
  key: string;
  scope?: CacheScope;
  sessionId?: string;
  /** TTL накопительного списка в Redis в секундах. */
  ttl: number;
  /** Номер запрашиваемой страницы (0-based). */
  page: number;
  /** Размер страницы который запрашивает клиент. */
  limit: number;
  /** Сколько элементов тянуть из upstream за раз. По умолчанию 200. */
  chunkSize?: number;
  /** Тянет следующий чанк из upstream используя предыдущий next_cursor. */
  fetcher: (cursor: string | undefined, chunkSize: number) => Promise<FetchChunkResult<T>>;
}

/**
 * Page-based пагинация поверх SC API с накопительным списком в Redis и singleflight.
 *
 * Идея: один ключ на ресурс (а не на курсор), элементы дотягиваются по мере необходимости.
 * Параллельные запросы за тем же ключом сливаются в один upstream-запрос.
 */
@Injectable()
export class ListCacheService {
  private readonly logger = new Logger(ListCacheService.name);
  private readonly inflight = new Map<string, Promise<void>>();

  constructor(
    @Inject(REDIS_CLIENT)
    private readonly redis: Redis,
  ) {}

  async getPage<T>(opts: GetPageOptions<T>): Promise<ListPageResult<T>> {
    const chunkSize = opts.chunkSize ?? DEFAULT_CHUNK_SIZE;
    const redisKey = this.buildRedisKey(opts.key, opts.scope ?? 'shared', opts.sessionId);
    const need = (opts.page + 1) * opts.limit;

    let state = await this.load<T>(redisKey);

    let chunks = 0;
    while (state.items.length < need && !state.exhausted && chunks < MAX_CHUNKS_PER_REQUEST) {
      await this.runOnce(redisKey, async () => {
        // re-read под локом чтобы не дублировать работу
        const fresh = await this.load<T>(redisKey);
        if (fresh.items.length >= need || fresh.exhausted) {
          state = fresh;
          return;
        }

        const fetched = await opts.fetcher(fresh.next_cursor, chunkSize);
        const items = fetched.items ?? [];

        const next: ListState<T> = {
          items: [...fresh.items, ...items],
          next_cursor: fetched.nextCursor,
          exhausted: !fetched.nextCursor || items.length === 0,
        };
        await this.save(redisKey, next, opts.ttl);
        state = next;
      });
      chunks++;
    }

    const start = opts.page * opts.limit;
    const slice = state.items.slice(start, start + opts.limit);
    const has_more = !state.exhausted || start + opts.limit < state.items.length;

    return {
      collection: slice,
      page: opts.page,
      page_size: opts.limit,
      has_more,
    };
  }

  async invalidate(key: string, scope: CacheScope = 'shared', sessionId?: string): Promise<void> {
    await this.redis.del(LIST_PREFIX + this.buildRedisKey(key, scope, sessionId)).catch(() => {});
  }

  async invalidateByCacheKeys(cacheKeys: string[], sessionId?: string): Promise<void> {
    const normalized = [...new Set(cacheKeys.map((k) => k.trim()).filter(Boolean))];
    if (normalized.length === 0) return;

    const redisKeys: string[] = [];
    for (const key of normalized) {
      redisKeys.push(LIST_PREFIX + this.buildRedisKey(key, 'shared'));
      if (sessionId) {
        redisKeys.push(LIST_PREFIX + this.buildRedisKey(key, 'user', sessionId));
      }
    }

    await this.redis.del(...redisKeys).catch((err) => {
      this.logger.warn(`list-cache invalidate failed: ${(err as Error).message}`);
    });
  }

  private buildRedisKey(key: string, scope: CacheScope, sessionId?: string): string {
    return scope === 'user' ? `user:${sessionId ?? ''}:${key}` : `shared:${key}`;
  }

  private async load<T>(redisKey: string): Promise<ListState<T>> {
    const raw = await this.redis.get(LIST_PREFIX + redisKey).catch((err) => {
      this.logger.warn(`list-cache get failed for ${redisKey}: ${(err as Error).message}`);
      return null;
    });
    if (!raw) return { items: [], next_cursor: undefined, exhausted: false };
    try {
      return JSON.parse(raw) as ListState<T>;
    } catch {
      this.redis.del(LIST_PREFIX + redisKey).catch(() => {});
      return { items: [], next_cursor: undefined, exhausted: false };
    }
  }

  private async save<T>(redisKey: string, state: ListState<T>, ttl: number): Promise<void> {
    await this.redis.set(LIST_PREFIX + redisKey, JSON.stringify(state), 'EX', ttl).catch((err) => {
      this.logger.warn(`list-cache set failed for ${redisKey}: ${(err as Error).message}`);
    });
  }

  private async runOnce(key: string, fn: () => Promise<void>): Promise<void> {
    const existing = this.inflight.get(key);
    if (existing) {
      await existing;
      return;
    }
    const p = fn().finally(() => {
      if (this.inflight.get(key) === p) this.inflight.delete(key);
    });
    this.inflight.set(key, p);
    await p;
  }
}

/**
 * Достаёт cursor из next_href SC API. SC обычно использует ?cursor=, для некоторых
 * эндпоинтов может быть ?offset=.
 */
export function extractScCursor(nextHref: string | undefined | null): string | undefined {
  if (!nextHref) return undefined;
  try {
    const url = new URL(nextHref);
    return url.searchParams.get('cursor') ?? url.searchParams.get('offset') ?? undefined;
  } catch {
    return undefined;
  }
}

/**
 * Стабильный cache-key из префикса ресурса и query-параметров, влияющих на upstream.
 * Игнорирует пустые значения и сортирует ключи, чтобы порядок не влиял.
 */
export function buildListCacheKey(prefix: string, params?: Record<string, unknown>): string {
  if (!params) return prefix;
  const parts = Object.entries(params)
    .filter(([, v]) => v != null && v !== '')
    .map(([k, v]) => `${k}=${v}`)
    .sort();
  return parts.length === 0 ? prefix : `${prefix}:${parts.join('&')}`;
}
