import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { OAuthAppsController } from './oauth-apps.controller.js';
import { OAuthAppsService } from './oauth-apps.service.js';

@Module({
  imports: [HttpModule],
  controllers: [OAuthAppsController],
  providers: [OAuthAppsService],
  exports: [OAuthAppsService],
})
export class OAuthAppsModule {}
