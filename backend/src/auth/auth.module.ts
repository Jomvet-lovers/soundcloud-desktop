import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { AuthController } from './auth.controller.js';
import { AuthService } from './auth.service.js';
import { Session } from './entities/session.entity.js';

@Module({
  imports: [TypeOrmModule.forFeature([Session]), SoundcloudModule],
  controllers: [AuthController],
  providers: [AuthService],
  exports: [AuthService],
})
export class AuthModule {}
