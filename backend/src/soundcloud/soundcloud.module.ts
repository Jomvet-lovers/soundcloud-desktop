import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { SoundcloudService } from './soundcloud.service.js';
import { SoundcloudListService } from './soundcloud-list.service.js';

@Module({
  imports: [HttpModule],
  providers: [SoundcloudService, SoundcloudListService],
  exports: [SoundcloudService, SoundcloudListService, HttpModule],
})
export class SoundcloudModule {}
