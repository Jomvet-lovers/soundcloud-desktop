import { Module } from '@nestjs/common';
import { QdrantModule } from '../qdrant/qdrant.module.js';
import { UserTasteService } from './user-taste.service.js';

@Module({
  imports: [QdrantModule],
  providers: [UserTasteService],
  exports: [UserTasteService],
})
export class UserTasteModule {}
