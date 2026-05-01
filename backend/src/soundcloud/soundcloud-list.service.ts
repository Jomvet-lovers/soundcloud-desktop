import { Injectable } from '@nestjs/common';
import {
  extractScCursor,
  ListCacheService,
  type ListPageResult,
} from '../cache/list-cache.service.js';
import { SoundcloudService } from './soundcloud.service.js';
import type { ScPaginatedResponse } from './soundcloud.types.js';

export interface ScPageOptions {
  /** Стабильный логический ключ ресурса. Например `related:{trackUrn}`. */
  cacheKey: string;
  /** TTL накопительного списка в секундах. */
  ttl: number;
  /** 'shared' (один кэш на всех) или 'user' (per-session). */
  scope?: 'shared' | 'user';
  sessionId?: string;
  /** Размер страницы для клиента. */
  limit: number;
  /** 0-based номер страницы. */
  page: number;
  /** SC API path: `/me/feed`, `/tracks/{urn}/related` и т.п. */
  path: string;
  /** OAuth access token. */
  token: string;
  /** Дополнительные query-параметры (access, и т.п.). НЕ должны включать limit/cursor/offset. */
  extraParams?: Record<string, unknown>;
  /** Сколько элементов тянуть из SC за один upstream-запрос. По умолчанию 200. */
  chunkSize?: number;
}

/**
 * Page-based пагинация поверх SC API. Использует ListCacheService для накопительного
 * кэша + singleflight, и SoundcloudService для апстрима.
 */
@Injectable()
export class SoundcloudListService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly listCache: ListCacheService,
  ) {}

  async getPage<T>(opts: ScPageOptions): Promise<ListPageResult<T>> {
    return this.listCache.getPage<T>({
      key: opts.cacheKey,
      scope: opts.scope ?? 'shared',
      sessionId: opts.sessionId,
      ttl: opts.ttl,
      page: opts.page,
      limit: opts.limit,
      chunkSize: opts.chunkSize,
      fetcher: async (cursor, chunkSize) => {
        const params: Record<string, unknown> = {
          ...(opts.extraParams ?? {}),
          limit: chunkSize,
          linked_partitioning: true,
        };
        if (cursor) params.cursor = cursor;

        const resp = await this.sc.apiGet<ScPaginatedResponse<T>>(opts.path, opts.token, params);
        return {
          items: resp.collection ?? [],
          nextCursor: extractScCursor(resp.next_href),
        };
      },
    });
  }
}
