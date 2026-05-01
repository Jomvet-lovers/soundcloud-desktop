import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { LocalLikesModule } from '../local-likes/local-likes.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { FeaturedController } from './featured.controller.js';
import { FeaturedService } from './featured.service.js';
import { FeaturedAdminController } from './featured-admin.controller.js';

@Module({
  imports: [SoundcloudModule, AuthModule, LocalLikesModule],
  controllers: [FeaturedController, FeaturedAdminController],
  providers: [FeaturedService],
})
export class FeaturedModule {}
