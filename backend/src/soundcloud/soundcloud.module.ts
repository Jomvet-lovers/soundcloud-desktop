import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { ScPublicApiService } from './sc-public-api.service.js';
import { SoundcloudService } from './soundcloud.service.js';

@Module({
  imports: [HttpModule],
  providers: [SoundcloudService, ScPublicApiService],
  exports: [SoundcloudService, ScPublicApiService],
})
export class SoundcloudModule {}
