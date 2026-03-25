import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { PendingActionsModule } from '../pending-actions/pending-actions.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { RepostsController } from './reposts.controller.js';
import { RepostsService } from './reposts.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule, PendingActionsModule],
  controllers: [RepostsController],
  providers: [RepostsService],
})
export class RepostsModule {}
