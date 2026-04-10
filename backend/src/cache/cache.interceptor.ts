import {
  type CallHandler,
  type ExecutionContext,
  Injectable,
  type NestInterceptor,
} from '@nestjs/common';
import { Reflector } from '@nestjs/core';
import { type Observable, of, tap } from 'rxjs';
import { CacheService } from './cache.service.js';
import { CACHE_CLEAR_OPTIONS_KEY, type CacheClearOptions } from './cache-clear.decorator.js';
import { CACHE_OPTIONS_KEY, type CachedOptions } from './cached.decorator.js';

@Injectable()
export class ApiCacheInterceptor implements NestInterceptor {
  constructor(
    private readonly reflector: Reflector,
    private readonly cacheService: CacheService,
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

    let key: string | null = null;
    if (cacheOptions) {
      key = this.cacheService.buildKey(method, url, cacheOptions.scope ?? 'shared', sessionId);

      const cached = await this.cacheService.get(key);
      if (cached !== null) return of(cached);
    }

    return next.handle().pipe(
      tap((payload) => {
        const statusCode = response?.statusCode ?? response?.raw?.statusCode ?? 200;
        if (statusCode >= 400) {
          return;
        }

        if (clearOptions?.keys.length) {
          this.cacheService.clearByCacheKeys(clearOptions.keys, sessionId).catch(() => {});
        }

        if (key && cacheOptions && payload !== undefined && payload !== null) {
          this.cacheService
            .set(key, payload, cacheOptions.ttl, {
              cacheKey: cacheOptions.key,
              scope: cacheOptions.scope ?? 'shared',
              sessionId,
            })
            .catch(() => {});
        }
      }),
    );
  }
}
