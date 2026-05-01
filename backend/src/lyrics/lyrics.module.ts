import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { GeniusService } from './genius.service.js';
import { LrclibService } from './lrclib.service.js';
import { LyricsController } from './lyrics.controller.js';
import { LyricsService } from './lyrics.service.js';
import { MusixmatchService } from './musixmatch.service.js';
import { WorkerClient } from './worker.client.js';

@Module({
  imports: [HttpModule],
  controllers: [LyricsController],
  providers: [LyricsService, LrclibService, MusixmatchService, GeniusService, WorkerClient],
  exports: [LyricsService, WorkerClient],
})
export class LyricsModule {}
