import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { PendingActionsModule } from '../pending-actions/pending-actions.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { PlaylistsController } from './playlists.controller.js';
import { PlaylistsService } from './playlists.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule, PendingActionsModule],
  controllers: [PlaylistsController],
  providers: [PlaylistsService],
})
export class PlaylistsModule {}
