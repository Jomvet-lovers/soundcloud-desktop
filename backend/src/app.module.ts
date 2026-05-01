import { Module } from '@nestjs/common';
import { ConfigModule, ConfigService } from '@nestjs/config';
import { ScheduleModule } from '@nestjs/schedule';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AdminStatsController } from './admin/admin-stats.controller.js';
import { AuthModule } from './auth/auth.module.js';
import { LinkRequest } from './auth/entities/link-request.entity.js';
import { LoginRequest } from './auth/entities/login-request.entity.js';
import { Session } from './auth/entities/session.entity.js';
import { NatsModule } from './bus/nats.module.js';
import { ApiCacheModule } from './cache/cache.module.js';
import { CdnTrack } from './cdn/entities/cdn-track.entity.js';
import { CollabModule } from './collab/collab.module.js';
import configuration from './config/configuration.js';
import { DislikesModule } from './dislikes/dislikes.module.js';
import { DislikedTrack } from './dislikes/entities/disliked-track.entity.js';
import { UserEvent } from './events/entities/user-event.entity.js';
import { EventsModule } from './events/events.module.js';
import { FeaturedItem } from './featured/entities/featured-item.entity.js';
import { FeaturedModule } from './featured/featured.module.js';
import { HealthController } from './health/health.controller.js';
import { ListeningHistory } from './history/entities/listening-history.entity.js';
import { HistoryModule } from './history/history.module.js';
import { IndexedTrack } from './indexing/entities/indexed-track.entity.js';
import { IndexingModule } from './indexing/indexing.module.js';
import { LikesModule } from './likes/likes.module.js';
import { LocalLike } from './local-likes/entities/local-like.entity.js';
import { LocalLikesModule } from './local-likes/local-likes.module.js';
import { LtrModule } from './ltr/ltr.module.js';
import { LyricsCache } from './lyrics/entities/lyrics-cache.entity.js';
import { LyricsModule } from './lyrics/lyrics.module.js';
import { MeModule } from './me/me.module.js';
import { OAuthApp } from './oauth-apps/entities/oauth-app.entity.js';
import { OAuthAppsModule } from './oauth-apps/oauth-apps.module.js';
import { PendingAction } from './pending-actions/entities/pending-action.entity.js';
import { PendingActionsModule } from './pending-actions/pending-actions.module.js';
import { PlaylistsModule } from './playlists/playlists.module.js';
import { QdrantModule } from './qdrant/qdrant.module.js';
import { RecommendationsModule } from './recommendations/recommendations.module.js';
import { RepostsModule } from './reposts/reposts.module.js';
import { ResolveModule } from './resolve/resolve.module.js';
import { SoundcloudModule } from './soundcloud/soundcloud.module.js';
import { Subscription } from './subscriptions/entities/subscription.entity.js';
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
    TypeOrmModule.forRootAsync({
      inject: [ConfigService],
      useFactory: (config: ConfigService) => ({
        type: 'postgres' as const,
        host: config.get<string>('database.host'),
        port: config.get<number>('database.port'),
        username: config.get<string>('database.username'),
        password: config.get<string>('database.password'),
        database: config.get<string>('database.name'),
        entities: [
          Session,
          LoginRequest,
          LinkRequest,
          ListeningHistory,
          LocalLike,
          OAuthApp,
          PendingAction,
          FeaturedItem,
          CdnTrack,
          Subscription,
          IndexedTrack,
          UserEvent,
          LyricsCache,
          DislikedTrack,
        ],
        synchronize: true,
      }),
    }),
    TypeOrmModule.forFeature([Session]),
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
