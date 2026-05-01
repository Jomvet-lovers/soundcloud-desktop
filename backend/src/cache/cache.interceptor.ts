import {
  type CallHandler,
  type ExecutionContext,
  Injectable,
  Logger,
  type NestInterceptor,
} from '@nestjs/common';
import { Reflector } from '@nestjs/core';
import { EMPTY, type Observable, tap } from 'rxjs';
import { CacheService } from './cache.service.js';
import { CACHE_CLEAR_OPTIONS_KEY, type CacheClearOptions } from './cache-clear.decorator.js';
import { CACHE_OPTIONS_KEY, type CachedOptions } from './cached.decorator.js';
import { ListCacheService } from './list-cache.service.js';

@Injectable()
export class ApiCacheInterceptor implements NestInterceptor {
  private readonly logger = new Logger(ApiCacheInterceptor.name);

  constructor(
    private readonly reflector: Reflector,
    private readonly cacheService: CacheService,
    private readonly listCacheService: ListCacheService,
  ) {}

  async intercept(context: ExecutionContext, next: CallHandler): Promise<Observable<unknown>> {
    const cacheOptions = this.reflector.get<CachedOptions | undefined>(
      CACHE_OPTIONS_KEY,
      context.getHandler(),
    );
    const clearOptions = this.reflector.get<CacheClearOptions | undefined>(
      CACHE_CLEAR_OPTIONS_KEY,
      context.getHandler(),
    );
    if (!cacheOptions && !clearOptions) return next.handle();

    const http = context.switchToHttp();
    const request = http.getRequest();
    const response = http.getResponse();
    const method = request.method ?? 'GET';
    const url = request.url ?? request.raw?.url ?? '';
    const sessionId = request.sessionId;

    const params = (request.params ?? {}) as Record<string, string | undefined>;
    const resolvedCacheKey = cacheOptions?.key
      ? resolveTemplate(cacheOptions.key, params)
      : undefined;
    const resolvedClearKeys = clearOptions?.keys.map((k) => resolveTemplate(k, params));

    let key: string | null = null;
    if (cacheOptions) {
      key = this.cacheService.buildKey(method, url, cacheOptions.scope ?? 'shared', sessionId);

      const raw = await this.cacheService.getRaw(key).catch((err) => {
        this.logger.warn(`cache get failed for ${method} ${url}: ${(err as Error).message}`);
        return null;
      });
      if (raw !== null) {
        // Fastify reply: hijack отнимает у NestJS право на повторный send,
        // иначе после нашего response.send(raw) фреймворк может попытаться
        // отдать undefined и Fastify бросит FST_ERR_REP_ALREADY_SENT в логи.
        if (typeof response.hijack === 'function') {
          response.hijack();
          // После hijack() Fastify больше не зовёт reply.send(), поэтому заголовки,
          // выставленные через reply.header(), не применяются — пишем напрямую в raw.
          response.raw.setHeader('content-type', 'application/json; charset=utf-8');
          response.raw.end(raw);
        } else if (typeof response.header === 'function') {
          response.header('content-type', 'application/json; charset=utf-8');
          response.send(raw);
        } else {
          response.setHeader?.('content-type', 'application/json; charset=utf-8');
          response.end?.(raw);
        }
        return EMPTY;
      }
    }

    return next.handle().pipe(
      tap((payload) => {
        const statusCode = response?.statusCode ?? response?.raw?.statusCode ?? 200;
        if (statusCode >= 400) return;

        if (resolvedClearKeys?.length) {
          this.cacheService.clearByCacheKeys(resolvedClearKeys, sessionId).catch((err) => {
            this.logger.warn(
              `cache clear failed for keys=${resolvedClearKeys.join(',')}: ${(err as Error).message}`,
            );
          });
          this.listCacheService.invalidateByCacheKeys(resolvedClearKeys, sessionId).catch((err) => {
            this.logger.warn(
              `list-cache clear failed for keys=${resolvedClearKeys.join(',')}: ${(err as Error).message}`,
            );
          });
        }

        if (key && cacheOptions && payload !== undefined && payload !== null) {
          this.cacheService
            .set(key, payload, cacheOptions.ttl, {
              cacheKey: resolvedCacheKey,
              scope: cacheOptions.scope ?? 'shared',
              sessionId,
            })
            .catch((err) => {
              this.logger.warn(`cache set failed for ${method} ${url}: ${(err as Error).message}`);
            });
        }
      }),
    );
  }
}

/**
 * Подставляет route params в плейсхолдеры `{name}`. Используется в `cacheOptions.key`
 * и `@CacheClear(...)` для точечной инвалидации (`playlist-detail:{playlistUrn}` →
 * `playlist-detail:soundcloud:playlists:42`).
 */
function resolveTemplate(template: string, params: Record<string, string | undefined>): string {
  return template.replace(/\{(\w+)\}/g, (_, name) => params[name] ?? `{${name}}`);
}
