import { createHash } from 'node:crypto';
import { Inject, Injectable, OnModuleDestroy } from '@nestjs/common';
import type { Redis } from 'ioredis';
import { REDIS_CLIENT } from './cache.constants.js';

type CacheScope = 'shared' | 'user';

const DATA_PREFIX = 'api:';
const INDEX_PREFIX = 'idx:';
const DEL_CHUNK = 500;

@Injectable()
export class CacheService implements OnModuleDestroy {
  constructor(
    @Inject(REDIS_CLIENT)
    private readonly redis: Redis,
  ) {}

  async onModuleDestroy() {
    try {
      await this.redis.quit();
    } catch {
      // ignore
    }
  }

  buildKey(method: string, url: string, scope: CacheScope, sessionId?: string): string {
    const [path, queryString] = url.split('?');
    const sortedQuery = queryString ? queryString.split('&').sort().join('&') : '';
    const raw =
      scope === 'user'
        ? `user:${method}:${path}:${sortedQuery}:${sessionId ?? ''}`
        : `shared:${method}:${path}:${sortedQuery}`;
    return createHash('sha256').update(raw).digest('hex');
  }

  async getRaw(key: string): Promise<string | null> {
    return this.redis.get(DATA_PREFIX + key);
  }

  async get(key: string): Promise<unknown | null> {
    const raw = await this.redis.get(DATA_PREFIX + key);
    if (!raw) return null;
    try {
      return JSON.parse(raw);
    } catch {
      this.redis.del(DATA_PREFIX + key).catch(() => {});
      return null;
    }
  }

  async set(
    key: string,
    response: unknown,
    ttlSeconds: number,
    options?: {
      cacheKey?: string;
      scope?: CacheScope;
      sessionId?: string;
    },
  ): Promise<void> {
    const dataKey = DATA_PREFIX + key;
    const payload = typeof response === 'string' ? response : JSON.stringify(response);
    const pipeline = this.redis.pipeline();
    pipeline.set(dataKey, payload, 'EX', ttlSeconds);

    if (options?.cacheKey) {
      const indexKey = this.buildIndexKey(
        options.cacheKey,
        options.scope ?? 'shared',
        options.sessionId,
      );
      const expireAt = Date.now() + ttlSeconds * 1000;
      pipeline.zadd(indexKey, expireAt, key);
      pipeline.zremrangebyscore(indexKey, 0, Date.now());
      pipeline.pexpireat(indexKey, expireAt);
    }

    await pipeline.exec();
  }

  async clearByCacheKeys(cacheKeys: string[], sessionId?: string): Promise<void> {
    const normalized = [...new Set(cacheKeys.map((k) => k.trim()).filter(Boolean))];
    if (normalized.length === 0) return;

    const indexKeys: string[] = [];
    for (const ck of normalized) {
      indexKeys.push(this.buildIndexKey(ck, 'shared'));
      if (sessionId) indexKeys.push(this.buildIndexKey(ck, 'user', sessionId));
    }

    await Promise.all(indexKeys.map((k) => this.clearIndex(k)));
  }

  private async clearIndex(indexKey: string): Promise<void> {
    const members = await this.redis.zrange(indexKey, 0, -1);
    if (members.length === 0) {
      await this.redis.del(indexKey);
      return;
    }

    const pipeline = this.redis.pipeline();
    for (let i = 0; i < members.length; i += DEL_CHUNK) {
      const chunk = members.slice(i, i + DEL_CHUNK).map((m) => DATA_PREFIX + m);
      pipeline.del(...chunk);
    }
    pipeline.del(indexKey);
    await pipeline.exec();
  }

  private buildIndexKey(cacheKey: string, scope: CacheScope, sessionId?: string): string {
    return scope === 'user'
      ? `${INDEX_PREFIX}user:${sessionId ?? ''}:${cacheKey}`
      : `${INDEX_PREFIX}shared:${cacheKey}`;
  }
}
