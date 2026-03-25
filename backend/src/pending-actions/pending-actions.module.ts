import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { OAuthAppsModule } from '../oauth-apps/oauth-apps.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { PendingAction } from './entities/pending-action.entity.js';
import { PendingActionsController } from './pending-actions.controller.js';
import { PendingActionsService } from './pending-actions.service.js';

@Module({
  imports: [
    TypeOrmModule.forFeature([PendingAction]),
    SoundcloudModule,
    AuthModule,
    OAuthAppsModule,
  ],
  controllers: [PendingActionsController],
  providers: [PendingActionsService],
  exports: [PendingActionsService],
})
export class PendingActionsModule {}
