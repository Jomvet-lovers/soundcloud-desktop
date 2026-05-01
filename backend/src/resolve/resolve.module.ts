import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { ResolveController } from './resolve.controller.js';
import { ResolveService } from './resolve.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule],
  controllers: [ResolveController],
  providers: [ResolveService],
})
export class ResolveModule {}
