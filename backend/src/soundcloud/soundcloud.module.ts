import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { SoundcloudService } from './soundcloud.service.js';

@Module({
  imports: [HttpModule],
  providers: [SoundcloudService],
  exports: [SoundcloudService],
})
export class SoundcloudModule {}
