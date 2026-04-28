import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { OAuthAppsModule } from '../oauth-apps/oauth-apps.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { AuthController } from './auth.controller.js';
import { AuthService } from './auth.service.js';
import { LinkRequest } from './entities/link-request.entity.js';
import { LoginRequest } from './entities/login-request.entity.js';
import { Session } from './entities/session.entity.js';
import { LinkController } from './link.controller.js';
import { LinkService } from './link.service.js';

@Module({
  imports: [
    TypeOrmModule.forFeature([Session, LoginRequest, LinkRequest]),
    SoundcloudModule,
    OAuthAppsModule,
  ],
  controllers: [AuthController, LinkController],
  providers: [AuthService, LinkService],
  exports: [AuthService],
})
export class AuthModule {}
