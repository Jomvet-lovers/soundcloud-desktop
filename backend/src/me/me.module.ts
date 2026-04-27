import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { EventsModule } from '../events/events.module.js';
import { LocalLikesModule } from '../local-likes/local-likes.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { SubscriptionsModule } from '../subscriptions/subscriptions.module.js';
import { MeController } from './me.controller.js';
import { MeService } from './me.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule, LocalLikesModule, SubscriptionsModule, EventsModule],
  controllers: [MeController],
  providers: [MeService],
})
export class MeModule {}
