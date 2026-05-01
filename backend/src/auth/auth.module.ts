import { Module } from '@nestjs/common';
import { OAuthAppsModule } from '../oauth-apps/oauth-apps.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { AuthController } from './auth.controller.js';
import { AuthService } from './auth.service.js';
import { LinkController } from './link.controller.js';
import { LinkService } from './link.service.js';

@Module({
  imports: [SoundcloudModule, OAuthAppsModule],
  controllers: [AuthController, LinkController],
  providers: [AuthService, LinkService],
  exports: [AuthService],
})
export class AuthModule {}
