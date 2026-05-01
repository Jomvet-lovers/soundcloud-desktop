import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { NatsModule } from '../bus/nats.module.js';
import { CentroidsModule } from '../centroids/centroids.module.js';
import { CollabModule } from '../collab/collab.module.js';
import { UserEvent } from '../events/entities/user-event.entity.js';
import { IndexedTrack } from '../indexing/entities/indexed-track.entity.js';
import { QdrantModule } from '../qdrant/qdrant.module.js';
import { LtrService } from './ltr.service.js';
import { LtrAdminController } from './ltr-admin.controller.js';
import { LtrTrainerService } from './ltr-trainer.service.js';

@Module({
  imports: [
    TypeOrmModule.forFeature([UserEvent, IndexedTrack]),
    QdrantModule,
    NatsModule,
    CollabModule,
    CentroidsModule,
  ],
  controllers: [LtrAdminController],
  providers: [LtrService, LtrTrainerService],
  exports: [LtrService],
})
export class LtrModule {}
