import { sql } from 'drizzle-orm';
import {
  bigint,
  boolean,
  doublePrecision,
  index,
  integer,
  jsonb,
  pgTable,
  real,
  text,
  timestamp,
  uniqueIndex,
  uuid,
  varchar,
} from 'drizzle-orm/pg-core';
import { v7 as uuidv7 } from 'uuid';

const uuidV7Pk = () => uuid('id').primaryKey().$defaultFn(uuidv7);
const createdAt = () => timestamp('created_at', { withTimezone: false }).notNull().defaultNow();
const updatedAt = () =>
  timestamp('updated_at', { withTimezone: false })
    .notNull()
    .defaultNow()
    .$onUpdate(() => new Date());

export const sessions = pgTable('sessions', {
  id: uuidV7Pk(),
  accessToken: text('access_token').notNull(),
  refreshToken: text('refresh_token').notNull(),
  expiresAt: timestamp('expires_at', { withTimezone: false }).notNull(),
  scope: text('scope').notNull(),
  soundcloudUserId: text('soundcloud_user_id'),
  username: text('username'),
  oauthAppId: text('oauth_app_id'),
  createdAt: createdAt(),
  updatedAt: updatedAt(),
});

export const loginRequests = pgTable(
  'login_requests',
  {
    id: uuidV7Pk(),
    state: text('state').notNull(),
    codeVerifier: text('code_verifier').notNull(),
    oauthAppId: text('oauth_app_id'),
    targetSessionId: uuid('target_session_id'),
    status: varchar('status', { length: 16 }).notNull().default('pending').$type<
      'pending' | 'processing' | 'completed' | 'failed'
    >(),
    resultSessionId: uuid('result_session_id'),
    error: text('error'),
    createdAt: createdAt(),
    expiresAt: timestamp('expires_at', { withTimezone: false }).notNull(),
  },
  (t) => [uniqueIndex('login_requests_state_uq').on(t.state)],
);

export const linkRequests = pgTable(
  'link_requests',
  {
    id: uuidV7Pk(),
    claimToken: text('claim_token').notNull(),
    mode: varchar('mode', { length: 8 }).notNull().$type<'pull' | 'push'>(),
    sourceSessionId: uuid('source_session_id'),
    targetSessionId: uuid('target_session_id'),
    status: varchar('status', { length: 16 }).notNull().default('pending').$type<
      'pending' | 'claimed' | 'failed'
    >(),
    error: text('error'),
    createdAt: createdAt(),
    expiresAt: timestamp('expires_at', { withTimezone: false }).notNull(),
  },
  (t) => [uniqueIndex('link_requests_claim_token_uq').on(t.claimToken)],
);

export const oauthApps = pgTable('oauth_apps', {
  id: uuidV7Pk(),
  name: text('name').notNull(),
  clientId: text('client_id').notNull(),
  clientSecret: text('client_secret').notNull(),
  redirectUri: text('redirect_uri').notNull(),
  active: boolean('active').notNull().default(true),
  lastUsedAt: timestamp('last_used_at', { withTimezone: true }),
  createdAt: createdAt(),
  updatedAt: updatedAt(),
});

export const cdnTracks = pgTable(
  'cdn_tracks',
  {
    id: uuidV7Pk(),
    trackUrn: text('track_urn').notNull(),
    quality: varchar('quality', { length: 4 }).notNull().$type<'hq' | 'sq'>(),
    cdnPath: text('cdn_path'),
    status: varchar('status', { length: 16 }).notNull().default('pending').$type<
      'pending' | 'ok' | 'error'
    >(),
    lastAccessedAt: timestamp('last_accessed_at', { withTimezone: true }).default(sql`NOW()`),
    createdAt: createdAt(),
    updatedAt: updatedAt(),
  },
  (t) => [
    index('cdn_tracks_track_urn_idx').on(t.trackUrn),
    index('cdn_tracks_status_idx').on(t.status),
    index('cdn_tracks_last_accessed_idx').on(t.lastAccessedAt),
    uniqueIndex('cdn_tracks_urn_quality_uq').on(t.trackUrn, t.quality),
  ],
);

export const dislikedTracks = pgTable(
  'disliked_tracks',
  {
    id: uuid('id').primaryKey().defaultRandom(),
    scUserId: text('sc_user_id').notNull(),
    scTrackId: text('sc_track_id').notNull(),
    trackData: jsonb('track_data').$type<Record<string, unknown> | null>(),
    createdAt: createdAt(),
  },
  (t) => [
    index('disliked_tracks_sc_user_id_idx').on(t.scUserId),
    index('disliked_tracks_sc_track_id_idx').on(t.scTrackId),
    uniqueIndex('disliked_tracks_user_track_uq').on(t.scUserId, t.scTrackId),
  ],
);

export const userEvents = pgTable(
  'user_events',
  {
    id: uuid('id').primaryKey().defaultRandom(),
    scUserId: text('sc_user_id').notNull(),
    scTrackId: text('sc_track_id').notNull(),
    eventType: text('event_type').notNull(),
    weight: doublePrecision('weight').notNull(),
    seeded: boolean('seeded').notNull().default(false),
    tasteAppliedAt: timestamp('taste_applied_at', { withTimezone: true }),
    createdAt: createdAt(),
  },
  (t) => [
    index('user_events_sc_user_id_idx').on(t.scUserId),
    index('user_events_taste_applied_at_idx').on(t.tasteAppliedAt),
  ],
);

export const featuredItems = pgTable('featured_items', {
  id: uuid('id').primaryKey().defaultRandom(),
  type: varchar('type', { length: 20 }).notNull().$type<'track' | 'playlist' | 'user'>(),
  scUrn: text('sc_urn').notNull(),
  weight: integer('weight').notNull().default(1),
  active: boolean('active').notNull().default(true),
  createdAt: createdAt(),
});

export const listeningHistory = pgTable(
  'listening_history',
  {
    id: uuid('id').primaryKey().defaultRandom(),
    soundcloudUserId: text('soundcloud_user_id').notNull(),
    scTrackId: text('sc_track_id').notNull(),
    title: text('title').notNull(),
    artistName: text('artist_name').notNull(),
    artistUrn: text('artist_urn'),
    artworkUrl: text('artwork_url'),
    duration: integer('duration').notNull(),
    playedAt: timestamp('played_at', { withTimezone: false }).notNull().defaultNow(),
  },
  (t) => [index('listening_history_user_id_idx').on(t.soundcloudUserId)],
);

export const indexedTracks = pgTable(
  'indexed_tracks',
  {
    id: uuid('id').primaryKey().defaultRandom(),
    scTrackId: text('sc_track_id').notNull(),
    title: text('title'),
    genre: text('genre'),
    tags: text('tags').array(),
    durationMs: integer('duration_ms'),
    artworkUrl: text('artwork_url'),
    streamUrl: text('stream_url'),
    rawScData: jsonb('raw_sc_data').$type<Record<string, unknown> | null>(),
    indexedAt: timestamp('indexed_at', { withTimezone: true }),
    language: varchar('language', { length: 8 }),
    languageConfidence: real('language_confidence'),
    s3VerifiedAt: timestamp('s3_verified_at', { withTimezone: true }),
    s3MissingAt: timestamp('s3_missing_at', { withTimezone: true }),
    createdAt: createdAt(),
  },
  (t) => [
    uniqueIndex('indexed_tracks_sc_track_id_uq').on(t.scTrackId),
    index('indexed_tracks_language_idx').on(t.language),
    index('indexed_tracks_s3_verified_at_idx').on(t.s3VerifiedAt),
    index('indexed_tracks_s3_missing_at_idx').on(t.s3MissingAt),
  ],
);

export const localLikes = pgTable(
  'local_likes',
  {
    id: uuid('id').primaryKey().defaultRandom(),
    soundcloudUserId: text('soundcloud_user_id').notNull(),
    scTrackId: text('sc_track_id').notNull(),
    trackData: jsonb('track_data').$type<Record<string, unknown>>().notNull(),
    createdAt: createdAt(),
  },
  (t) => [
    index('local_likes_user_id_idx').on(t.soundcloudUserId),
    uniqueIndex('local_likes_user_track_uq').on(t.soundcloudUserId, t.scTrackId),
  ],
);

export const lyricsCache = pgTable(
  'lyrics_cache',
  {
    scTrackId: text('sc_track_id').primaryKey(),
    syncedLrc: text('synced_lrc'),
    plainText: text('plain_text'),
    source: varchar('source', { length: 16 })
      .notNull()
      .$type<'lrclib' | 'musixmatch' | 'lyricsovh' | 'genius' | 'textyl' | 'self_gen' | 'none'>(),
    language: varchar('language', { length: 8 }),
    languageConfidence: real('language_confidence'),
    embeddedAt: timestamp('embedded_at', { withTimezone: true }),
    createdAt: createdAt(),
  },
  (t) => [index('lyrics_cache_language_idx').on(t.language)],
);

export const pendingActions = pgTable(
  'pending_actions',
  {
    id: uuidV7Pk(),
    sessionId: text('session_id').notNull(),
    actionType: varchar('action_type', { length: 32 }).notNull(),
    targetUrn: text('target_urn').notNull(),
    payload: jsonb('payload').$type<Record<string, unknown> | null>(),
    status: varchar('status', { length: 16 }).notNull().default('pending').$type<
      'pending' | 'done' | 'failed'
    >(),
    error: text('error'),
    retryCount: integer('retry_count').notNull().default(0),
    createdAt: createdAt(),
    updatedAt: updatedAt(),
  },
  (t) => [index('pending_actions_session_id_idx').on(t.sessionId)],
);

export const subscriptions = pgTable('subscriptions', {
  userUrn: text('user_urn').primaryKey(),
  expDate: bigint('exp_date', { mode: 'number' }).notNull(),
});

export type Session = typeof sessions.$inferSelect;
export type NewSession = typeof sessions.$inferInsert;
export type LoginRequest = typeof loginRequests.$inferSelect;
export type NewLoginRequest = typeof loginRequests.$inferInsert;
export type LinkRequest = typeof linkRequests.$inferSelect;
export type NewLinkRequest = typeof linkRequests.$inferInsert;
export type OAuthApp = typeof oauthApps.$inferSelect;
export type NewOAuthApp = typeof oauthApps.$inferInsert;
export type CdnTrack = typeof cdnTracks.$inferSelect;
export type NewCdnTrack = typeof cdnTracks.$inferInsert;
export type DislikedTrack = typeof dislikedTracks.$inferSelect;
export type NewDislikedTrack = typeof dislikedTracks.$inferInsert;
export type UserEvent = typeof userEvents.$inferSelect;
export type NewUserEvent = typeof userEvents.$inferInsert;
export type FeaturedItem = typeof featuredItems.$inferSelect;
export type NewFeaturedItem = typeof featuredItems.$inferInsert;
export type ListeningHistory = typeof listeningHistory.$inferSelect;
export type NewListeningHistory = typeof listeningHistory.$inferInsert;
export type IndexedTrack = typeof indexedTracks.$inferSelect;
export type NewIndexedTrack = typeof indexedTracks.$inferInsert;
export type LocalLike = typeof localLikes.$inferSelect;
export type NewLocalLike = typeof localLikes.$inferInsert;
export type LyricsCache = typeof lyricsCache.$inferSelect;
export type NewLyricsCache = typeof lyricsCache.$inferInsert;
export type PendingAction = typeof pendingActions.$inferSelect;
export type NewPendingAction = typeof pendingActions.$inferInsert;
export type Subscription = typeof subscriptions.$inferSelect;
export type NewSubscription = typeof subscriptions.$inferInsert;
