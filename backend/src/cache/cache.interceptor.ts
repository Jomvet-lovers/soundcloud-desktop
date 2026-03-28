import {
  type CallHandler,
  type ExecutionContext,
  Injectable,
  type NestInterceptor,
} from '@nestjs/common';
import { Reflector } from '@nestjs/core';
import { type Observable, of, tap } from 'rxjs';
import { CacheService } from './cache.service.js';
import { CACHE_OPTIONS_KEY, type CachedOptions } from './cached.decorator.js';

@Injectable()
export class ApiCacheInterceptor implements NestInterceptor {
  constructor(
    private readonly reflector: Reflector,
    private readonly cacheService: CacheService,
  ) {}

  async intercept(context: ExecutionContext, next: CallHandler): Promise<Observable<unknown>> {
    const options = this.reflector.get<CachedOptions | undefined>(
      CACHE_OPTIONS_KEY,
      context.getHandler(),
    );
    if (!options) return next.handle();

    const request = context.switchToHttp().getRequest();
    const method = request.method ?? 'GET';
    const url = request.url ?? request.raw?.url ?? '';
    const sessionId = request.sessionId;

    const key = this.cacheService.buildKey(method, url, options.scope ?? 'shared', sessionId);

    const cached = await this.cacheService.get(key);
    if (cached !== null) return of(cached);

    return next.handle().pipe(
      tap((response) => {
        if (response !== undefined && response !== null) {
          this.cacheService.set(key, response, options.ttl).catch(() => {});
        }
      }),
    );
  }
}
