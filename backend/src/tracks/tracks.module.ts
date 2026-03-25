import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { CdnModule } from '../cdn/cdn.module.js';
import { LocalLikesModule } from '../local-likes/local-likes.module.js';
import { PendingActionsModule } from '../pending-actions/pending-actions.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { TracksController } from './tracks.controller.js';
import { TracksService } from './tracks.service.js';

@Module({
  imports: [HttpModule, SoundcloudModule, AuthModule, LocalLikesModule, CdnModule, PendingActionsModule],
  controllers: [TracksController],
  providers: [TracksService],
})
export class TracksModule {}
