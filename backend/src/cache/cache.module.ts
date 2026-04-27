import { Global, Module } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { APP_INTERCEPTOR } from '@nestjs/core';
import { Redis } from 'ioredis';
import { REDIS_CLIENT } from './cache.constants.js';
import { ApiCacheInterceptor } from './cache.interceptor.js';
import { CacheService } from './cache.service.js';

@Global()
@Module({
  providers: [
    {
      provide: REDIS_CLIENT,
      inject: [ConfigService],
      useFactory: (config: ConfigService) => {
        const url = config.get<string>('redis.url') || 'redis://localhost:6379';
        return new Redis(url, {
          maxRetriesPerRequest: 3,
          enableAutoPipelining: true,
          connectTimeout: 5000,
          keepAlive: 30000,
        });
      },
    },
    CacheService,
    ApiCacheInterceptor,
    {
      provide: APP_INTERCEPTOR,
      useExisting: ApiCacheInterceptor,
    },
  ],
  exports: [CacheService, REDIS_CLIENT],
})
export class ApiCacheModule {}
