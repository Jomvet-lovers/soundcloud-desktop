import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { LikesController } from './likes.controller.js';
import { LikesService } from './likes.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule],
  controllers: [LikesController],
  providers: [LikesService],
})
export class LikesModule {}
