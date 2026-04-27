import type { CallHandler, ExecutionContext } from '@nestjs/common';
import { Reflector } from '@nestjs/core';
import { lastValueFrom, of } from 'rxjs';
import { ApiCacheInterceptor } from './cache.interceptor';
import { CacheService } from './cache.service';
import { CACHE_CLEAR_OPTIONS_KEY } from './cache-clear.decorator';
import { CACHE_OPTIONS_KEY } from './cached.decorator';

describe('ApiCacheInterceptor', () => {
  const handler = () => undefined;

  function createContext(statusCode = 200, reply?: any): ExecutionContext {
    return {
      getHandler: () => handler,
      switchToHttp: () => ({
        getRequest: () => ({
          method: 'GET',
          url: '/tracks/123',
          sessionId: 'session-1',
        }),
        getResponse: () => reply ?? { statusCode },
      }),
    } as ExecutionContext;
  }

  function createNext(payload: unknown): CallHandler {
    return {
      handle: () => of(payload),
    };
  }

  it('caches successful responses', async () => {
    const reflector = {
      get: jest.fn((metadataKey: string) => {
        if (metadataKey === CACHE_OPTIONS_KEY) {
          return { ttl: 60, scope: 'shared', key: 'tracks' };
        }
        return undefined;
      }),
    } as unknown as Reflector;
    const cacheService = {
      buildKey: jest.fn().mockReturnValue('cache-key'),
      getRaw: jest.fn().mockResolvedValue(null),
      set: jest.fn().mockResolvedValue(undefined),
      clearByCacheKeys: jest.fn().mockResolvedValue(undefined),
    } as unknown as CacheService;
    const interceptor = new ApiCacheInterceptor(reflector, cacheService);
    const payload = { ok: true };

    const result = await lastValueFrom(
      await interceptor.intercept(createContext(200), createNext(payload)),
    );

    expect(result).toEqual(payload);
    expect(cacheService.set).toHaveBeenCalledWith('cache-key', payload, 60, {
      cacheKey: 'tracks',
      scope: 'shared',
      sessionId: 'session-1',
    });
  });

  it('does not cache error responses', async () => {
    const reflector = {
      get: jest.fn((metadataKey: string) => {
        if (metadataKey === CACHE_OPTIONS_KEY) {
          return { ttl: 60, scope: 'shared', key: 'tracks' };
        }
        return undefined;
      }),
    } as unknown as Reflector;
    const cacheService = {
      buildKey: jest.fn().mockReturnValue('cache-key'),
      getRaw: jest.fn().mockResolvedValue(null),
      set: jest.fn().mockResolvedValue(undefined),
      clearByCacheKeys: jest.fn().mockResolvedValue(undefined),
    } as unknown as CacheService;
    const interceptor = new ApiCacheInterceptor(reflector, cacheService);

    await lastValueFrom(await interceptor.intercept(createContext(404), createNext({})));

    expect(cacheService.set).not.toHaveBeenCalled();
  });

  it('sends raw cached payload and skips handler', async () => {
    const reflector = {
      get: jest.fn((metadataKey: string) => {
        if (metadataKey === CACHE_OPTIONS_KEY) {
          return { ttl: 60, scope: 'shared', key: 'tracks' };
        }
        return undefined;
      }),
    } as unknown as Reflector;
    const cacheService = {
      buildKey: jest.fn().mockReturnValue('cache-key'),
      getRaw: jest.fn().mockResolvedValue('{"cached":true}'),
      set: jest.fn(),
      clearByCacheKeys: jest.fn().mockResolvedValue(undefined),
    } as unknown as CacheService;
    const interceptor = new ApiCacheInterceptor(reflector, cacheService);
    const next = {
      handle: jest.fn().mockReturnValue(of({ fresh: true })),
    } as unknown as CallHandler;
    const reply = {
      statusCode: 200,
      header: jest.fn(),
      send: jest.fn(),
    };

    const observable = await interceptor.intercept(createContext(200, reply), next);
    await observable.toPromise?.();

    expect(next.handle).not.toHaveBeenCalled();
    expect(reply.header).toHaveBeenCalledWith('content-type', 'application/json; charset=utf-8');
    expect(reply.send).toHaveBeenCalledWith('{"cached":true}');
  });

  it('clears named caches after successful mutation', async () => {
    const reflector = {
      get: jest.fn((metadataKey: string) => {
        if (metadataKey === CACHE_CLEAR_OPTIONS_KEY) {
          return { keys: ['me-liked-tracks', 'me-liked-playlists'] };
        }
        return undefined;
      }),
    } as unknown as Reflector;
    const cacheService = {
      buildKey: jest.fn(),
      getRaw: jest.fn(),
      set: jest.fn(),
      clearByCacheKeys: jest.fn().mockResolvedValue(undefined),
    } as unknown as CacheService;
    const interceptor = new ApiCacheInterceptor(reflector, cacheService);

    await lastValueFrom(await interceptor.intercept(createContext(200), createNext({ ok: true })));

    expect(cacheService.clearByCacheKeys).toHaveBeenCalledWith(
      ['me-liked-tracks', 'me-liked-playlists'],
      'session-1',
    );
    expect(cacheService.set).not.toHaveBeenCalled();
  });

  it('does not clear named caches after error response', async () => {
    const reflector = {
      get: jest.fn((metadataKey: string) => {
        if (metadataKey === CACHE_CLEAR_OPTIONS_KEY) {
          return { keys: ['me-liked-tracks'] };
        }
        return undefined;
      }),
    } as unknown as Reflector;
    const cacheService = {
      buildKey: jest.fn(),
      getRaw: jest.fn(),
      set: jest.fn(),
      clearByCacheKeys: jest.fn().mockResolvedValue(undefined),
    } as unknown as CacheService;
    const interceptor = new ApiCacheInterceptor(reflector, cacheService);

    await lastValueFrom(await interceptor.intercept(createContext(409), createNext({})));

    expect(cacheService.clearByCacheKeys).not.toHaveBeenCalled();
  });
});
