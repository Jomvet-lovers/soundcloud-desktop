import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { TracksController } from './tracks.controller.js';
import { TracksService } from './tracks.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule],
  controllers: [TracksController],
  providers: [TracksService],
})
export class TracksModule {}
