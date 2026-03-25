import { Module } from '@nestjs/common';
import { ConfigModule, ConfigService } from '@nestjs/config';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from './auth/auth.module.js';
import { Session } from './auth/entities/session.entity.js';
import configuration from './config/configuration.js';
import { HealthController } from './health/health.controller.js';
import { ListeningHistory } from './history/entities/listening-history.entity.js';
import { HistoryModule } from './history/history.module.js';
import { LikesModule } from './likes/likes.module.js';
import { LocalLike } from './local-likes/entities/local-like.entity.js';
import { LocalLikesModule } from './local-likes/local-likes.module.js';
import { MeModule } from './me/me.module.js';
import { CdnModule } from './cdn/cdn.module.js';
import { OAuthApp } from './oauth-apps/entities/oauth-app.entity.js';
import { OAuthAppsModule } from './oauth-apps/oauth-apps.module.js';
import { PendingAction } from './pending-actions/entities/pending-action.entity.js';
import { PendingActionsModule } from './pending-actions/pending-actions.module.js';
import { PlaylistsModule } from './playlists/playlists.module.js';
import { RepostsModule } from './reposts/reposts.module.js';
import { ResolveModule } from './resolve/resolve.module.js';
import { SoundcloudModule } from './soundcloud/soundcloud.module.js';
import { TracksModule } from './tracks/tracks.module.js';
import { UsersModule } from './users/users.module.js';

@Module({
  imports: [
    ConfigModule.forRoot({
      isGlobal: true,
      load: [configuration],
    }),
    TypeOrmModule.forRootAsync({
      inject: [ConfigService],
      useFactory: (config: ConfigService) => ({
        type: 'postgres' as const,
        host: config.get<string>('database.host'),
        port: config.get<number>('database.port'),
        username: config.get<string>('database.username'),
        password: config.get<string>('database.password'),
        database: config.get<string>('database.name'),
        entities: [Session, ListeningHistory, LocalLike, OAuthApp, PendingAction],
        synchronize: true,
      }),
    }),
    OAuthAppsModule,
    AuthModule,
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
  ],
  controllers: [HealthController],
})
export class AppModule {}
