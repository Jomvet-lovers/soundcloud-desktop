import { Global, Module } from '@nestjs/common';
import { APP_INTERCEPTOR } from '@nestjs/core';
import { TypeOrmModule } from '@nestjs/typeorm';
import { ApiCacheInterceptor } from './cache.interceptor.js';
import { CacheService } from './cache.service.js';
import { ApiCache } from './entities/api-cache.entity.js';

@Global()
@Module({
  imports: [TypeOrmModule.forFeature([ApiCache])],
  providers: [
    CacheService,
    ApiCacheInterceptor,
    {
      provide: APP_INTERCEPTOR,
      useExisting: ApiCacheInterceptor,
    },
  ],
  exports: [CacheService],
})
export class ApiCacheModule {}
