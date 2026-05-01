import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { ScheduleModule } from '@nestjs/schedule';
import { AdminStatsController } from './admin/admin-stats.controller.js';
import { AuthModule } from './auth/auth.module.js';
import { NatsModule } from './bus/nats.module.js';
import { ApiCacheModule } from './cache/cache.module.js';
import { CollabModule } from './collab/collab.module.js';
import { CronLeaderModule } from './common/cron-leader/cron-leader.module.js';
import configuration from './config/configuration.js';
import { DbModule } from './db/db.module.js';
import { DislikesModule } from './dislikes/dislikes.module.js';
import { EventsModule } from './events/events.module.js';
import { FeaturedModule } from './featured/featured.module.js';
import { HealthController } from './health/health.controller.js';
import { HistoryModule } from './history/history.module.js';
import { IndexingModule } from './indexing/indexing.module.js';
import { LikesModule } from './likes/likes.module.js';
import { LocalLikesModule } from './local-likes/local-likes.module.js';
import { LtrModule } from './ltr/ltr.module.js';
import { LyricsModule } from './lyrics/lyrics.module.js';
import { MeModule } from './me/me.module.js';
import { OAuthAppsModule } from './oauth-apps/oauth-apps.module.js';
import { PendingActionsModule } from './pending-actions/pending-actions.module.js';
import { PlaylistsModule } from './playlists/playlists.module.js';
import { QdrantModule } from './qdrant/qdrant.module.js';
import { RecommendationsModule } from './recommendations/recommendations.module.js';
import { RepostsModule } from './reposts/reposts.module.js';
import { ResolveModule } from './resolve/resolve.module.js';
import { SoundcloudModule } from './soundcloud/soundcloud.module.js';
import { SubscriptionsModule } from './subscriptions/subscriptions.module.js';
import { TracksModule } from './tracks/tracks.module.js';
import { TranscodeModule } from './transcode/transcode.module.js';
import { UserTasteModule } from './user-taste/user-taste.module.js';
import { UsersModule } from './users/users.module.js';

@Module({
  imports: [
    ConfigModule.forRoot({
      isGlobal: true,
      load: [configuration],
    }),
    ScheduleModule.forRoot(),
    DbModule,
    CronLeaderModule,
    NatsModule,
    TranscodeModule,
    QdrantModule,
    IndexingModule,
    ApiCacheModule,
    OAuthAppsModule,
    AuthModule,
    FeaturedModule,
    SoundcloudModule,
    MeModule,
    TracksModule,
    PlaylistsModule,
    UsersModule,
    LikesModule,
    RepostsModule,
    ResolveModule,
    HistoryModule,
    LocalLikesModule,
    PendingActionsModule,
    SubscriptionsModule,
    UserTasteModule,
    EventsModule,
    RecommendationsModule,
    LyricsModule,
    DislikesModule,
    CollabModule,
    LtrModule,
  ],
  controllers: [HealthController, AdminStatsController],
})
export class AppModule {}
