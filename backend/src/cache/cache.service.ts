import { createHash } from 'node:crypto';
import { Injectable, Logger, OnModuleDestroy, OnModuleInit } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { LessThan, Repository } from 'typeorm';
import { ApiCache } from './entities/api-cache.entity.js';

@Injectable()
export class CacheService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(CacheService.name);
  private cleanupTimer: NodeJS.Timeout | null = null;
  private static readonly CLEANUP_INTERVAL_MS = 60_000;

  constructor(
    @InjectRepository(ApiCache)
    private readonly repo: Repository<ApiCache>,
  ) {}

  onModuleInit() {
    this.cleanupTimer = setInterval(() => this.cleanup(), CacheService.CLEANUP_INTERVAL_MS);
  }

  onModuleDestroy() {
    if (this.cleanupTimer) clearInterval(this.cleanupTimer);
  }

  /** Строит cache key: SHA-256(scope:METHOD:path?sorted_query[:sessionId]) */
  buildKey(method: string, url: string, scope: 'shared' | 'user', sessionId?: string): string {
    const [path, queryString] = url.split('?');
    const sortedQuery = queryString ? queryString.split('&').sort().join('&') : '';
    const raw =
      scope === 'user'
        ? `user:${method}:${path}:${sortedQuery}:${sessionId ?? ''}`
        : `shared:${method}:${path}:${sortedQuery}`;
    return createHash('sha256').update(raw).digest('hex');
  }

  async get(key: string): Promise<unknown | null> {
    const entry = await this.repo.findOne({ where: { key } });
    if (!entry) return null;
    if (entry.expiresAt <= new Date()) {
      await this.repo.delete(key);
      return null;
    }
    return entry.response;
  }

  async set(key: string, response: unknown, ttlSeconds: number): Promise<void> {
    const expiresAt = new Date(Date.now() + ttlSeconds * 1000);
    await this.repo.upsert({ key, response: response as Record<string, unknown>, expiresAt }, [
      'key',
    ]);
  }

  private async cleanup(): Promise<void> {
    try {
      const { affected } = await this.repo.delete({ expiresAt: LessThan(new Date()) });
      if (affected && affected > 0) {
        this.logger.debug(`Cache cleanup: removed ${affected} expired entries`);
      }
    } catch (err: any) {
      this.logger.warn(`Cache cleanup error: ${err.message}`);
    }
  }
}
