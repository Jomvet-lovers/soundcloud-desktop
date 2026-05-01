import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { OAuthAppsModule } from '../oauth-apps/oauth-apps.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { PendingActionsController } from './pending-actions.controller.js';
import { PendingActionsService } from './pending-actions.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule, OAuthAppsModule],
  controllers: [PendingActionsController],
  providers: [PendingActionsService],
  exports: [PendingActionsService],
})
export class PendingActionsModule {}
