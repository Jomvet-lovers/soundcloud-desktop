import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { CdnService } from './cdn.service.js';
import { CdnTrack } from './entities/cdn-track.entity.js';

@Module({
  imports: [HttpModule, TypeOrmModule.forFeature([CdnTrack])],
  providers: [CdnService],
  exports: [CdnService],
})
export class CdnModule {}
