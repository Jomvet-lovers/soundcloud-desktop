import { Module } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { QdrantClient } from '@qdrant/js-client-rest';
import { QdrantInitService } from './qdrant-init.service.js';

@Module({
  providers: [
    {
      provide: 'QDRANT_CLIENT',
      inject: [ConfigService],
      useFactory: (config: ConfigService) =>
        new QdrantClient({
          url: config.get<string>('qdrant.url'),
          apiKey: config.get<string>('qdrant.apiKey') || undefined,
        }),
    },
    QdrantInitService,
  ],
  exports: ['QDRANT_CLIENT'],
})
export class QdrantModule {}
