import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { CentroidsModule } from '../centroids/centroids.module.js';
import { CollabModule } from '../collab/collab.module.js';
import { DislikesModule } from '../dislikes/dislikes.module.js';
import { EventsModule } from '../events/events.module.js';
import { LtrModule } from '../ltr/ltr.module.js';
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
    CollabModule,
    CentroidsModule,
    LtrModule,
  ],
  controllers: [RecommendationsController],
  providers: [RecommendationsService, S3VerifierService],
})
export class RecommendationsModule {}
