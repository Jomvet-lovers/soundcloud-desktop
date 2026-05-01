import { Module } from '@nestjs/common';
import { QdrantModule } from '../qdrant/qdrant.module.js';
import { CentroidService } from './centroid.service.js';

@Module({
  imports: [QdrantModule],
  providers: [CentroidService],
  exports: [CentroidService],
})
export class CentroidsModule {}
