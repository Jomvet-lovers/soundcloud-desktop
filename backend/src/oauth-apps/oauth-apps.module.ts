import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { OAuthApp } from './entities/oauth-app.entity.js';
import { OAuthAppsController } from './oauth-apps.controller.js';
import { OAuthAppsService } from './oauth-apps.service.js';

@Module({
  imports: [TypeOrmModule.forFeature([OAuthApp]), HttpModule],
  controllers: [OAuthAppsController],
  providers: [OAuthAppsService],
  exports: [OAuthAppsService],
})
export class OAuthAppsModule {}
