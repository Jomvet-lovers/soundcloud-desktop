import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { RepostsController } from './reposts.controller.js';
import { RepostsService } from './reposts.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule],
  controllers: [RepostsController],
  providers: [RepostsService],
})
export class RepostsModule {}
