import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { LocalLikesModule } from '../local-likes/local-likes.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { LikesController } from './likes.controller.js';
import { LikesService } from './likes.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule, LocalLikesModule],
  controllers: [LikesController],
  providers: [LikesService],
})
export class LikesModule {}
