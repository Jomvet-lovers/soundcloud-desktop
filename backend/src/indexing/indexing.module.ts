import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { LyricsModule } from '../lyrics/lyrics.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { IndexedTrack } from './entities/indexed-track.entity.js';
import { IndexingController } from './indexing.controller.js';
import { IndexingService } from './indexing.service.js';
import { TrackDiscoveryService } from './track-discovery.service.js';

@Module({
  imports: [
    HttpModule,
    TypeOrmModule.forFeature([IndexedTrack]),
    SoundcloudModule,
    AuthModule,
    LyricsModule,
  ],
  controllers: [IndexingController],
  providers: [IndexingService, TrackDiscoveryService],
  exports: [IndexingService, TrackDiscoveryService],
})
export class IndexingModule {}
