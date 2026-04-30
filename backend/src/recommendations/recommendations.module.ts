import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { DislikesModule } from '../dislikes/dislikes.module.js';
import { EventsModule } from '../events/events.module.js';
import { IndexedTrack } from '../indexing/entities/indexed-track.entity.js';
import { LyricsModule } from '../lyrics/lyrics.module.js';
import { QdrantModule } from '../qdrant/qdrant.module.js';
import { RecommendationsController } from './recommendations.controller.js';
import { RecommendationsService } from './recommendations.service.js';
import { S3VerifierService } from './s3-verifier.service.js';

@Module({
  imports: [
    HttpModule,
    QdrantModule,
    AuthModule,
    EventsModule,
    DislikesModule,
    LyricsModule,
    TypeOrmModule.forFeature([IndexedTrack]),
  ],
  controllers: [RecommendationsController],
  providers: [RecommendationsService, S3VerifierService],
})
export class RecommendationsModule {}
