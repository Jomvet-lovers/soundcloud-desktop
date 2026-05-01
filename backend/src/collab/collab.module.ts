import { Module } from '@nestjs/common';
import { NatsModule } from '../bus/nats.module.js';
import { QdrantModule } from '../qdrant/qdrant.module.js';
import { CollabAdminController } from './collab-admin.controller.js';
import { CollabTrainerService } from './collab-trainer.service.js';
import { CollabVectorService } from './collab-vector.service.js';

@Module({
  imports: [QdrantModule, NatsModule],
  controllers: [CollabAdminController],
  providers: [CollabVectorService, CollabTrainerService],
  exports: [CollabVectorService, CollabTrainerService],
})
export class CollabModule {}
