import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { Session } from '../auth/entities/session.entity.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { ResolveController } from './resolve.controller.js';
import { ResolveService } from './resolve.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule, TypeOrmModule.forFeature([Session])],
  controllers: [ResolveController],
  providers: [ResolveService],
})
export class ResolveModule {}
