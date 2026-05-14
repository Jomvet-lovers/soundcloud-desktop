-- Phase 2: cold-cache. Per-user state-mirror + shared entity caches.
-- Запись: оптимистичная (пишем сразу в локальные таблицы + sync_queue).
-- Чтение: только из PG (SWR/refresh — Phase 3).
-- progress=true означает "локальное состояние ещё не подтверждено SC".

-- local_likes снесён — заменён на user_likes_tracks (см. ниже).
DROP TABLE IF EXISTS "local_likes";

ALTER TABLE "indexed_tracks" ADD COLUMN "synced_at"    timestamp with time zone;
ALTER TABLE "indexed_tracks" ADD COLUMN "last_read_at" timestamp with time zone;
CREATE INDEX "indexed_tracks_synced_at_idx"    ON "indexed_tracks" ("synced_at");
CREATE INDEX "indexed_tracks_last_read_at_idx" ON "indexed_tracks" ("last_read_at");

-- =========================================================================
-- User-state: что юзер залайкал / репостнул / зафоловил / создал.
-- =========================================================================

CREATE TABLE "user_likes_tracks" (
    "user_id"      text NOT NULL,
    "sc_track_id"  text NOT NULL,
    "wanted_state" boolean NOT NULL DEFAULT true,
    "progress"     boolean NOT NULL DEFAULT false,
    "synced_at"    timestamp with time zone,
    "last_read_at" timestamp with time zone,
    "created_at"   timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY ("user_id", "sc_track_id")
);
CREATE INDEX "user_likes_tracks_user_created_idx"
    ON "user_likes_tracks" ("user_id", "created_at" DESC);

CREATE TABLE "user_likes_playlists" (
    "user_id"      text NOT NULL,
    "playlist_urn" text NOT NULL,
    "wanted_state" boolean NOT NULL DEFAULT true,
    "progress"     boolean NOT NULL DEFAULT false,
    "synced_at"    timestamp with time zone,
    "last_read_at" timestamp with time zone,
    "created_at"   timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY ("user_id", "playlist_urn")
);
CREATE INDEX "user_likes_playlists_user_created_idx"
    ON "user_likes_playlists" ("user_id", "created_at" DESC);

CREATE TABLE "user_followings" (
    "user_id"         text NOT NULL,
    "target_user_urn" text NOT NULL,
    "wanted_state"    boolean NOT NULL DEFAULT true,
    "progress"        boolean NOT NULL DEFAULT false,
    "synced_at"       timestamp with time zone,
    "last_read_at"    timestamp with time zone,
    "created_at"      timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY ("user_id", "target_user_urn")
);
CREATE INDEX "user_followings_user_created_idx"
    ON "user_followings" ("user_id", "created_at" DESC);

-- Owned playlists/tracks — без wanted_state: создание и удаление здесь
-- симметричны, "intent on deletion" представляется отсутствием строки
-- + queue(playlist_delete). До успешного синка строка остаётся в таблице
-- (progress=true), refresh её не трогает.
--
-- `payload` хранит ПРИВАТНЫЙ subset, который SC отдаёт только владельцу через
-- `/me/{tracks,playlists}`. В indexed_tracks/cached_playlists, которые читают
-- все клиенты, эти приватные данные класть НЕЛЬЗЯ — иначе любой запрос
-- `/tracks/{urn}` или `/playlists/{urn}` к нашему трeку покажет sharing=private
-- payload чужому. Публичные owned-сущности всё равно дублируются в общие
-- кеши, чтобы read-path другого клиента находил их без обращения к SC.
CREATE TABLE "user_owned_playlists" (
    "user_id"      text NOT NULL,
    "playlist_urn" text NOT NULL,
    "payload"      jsonb,
    "progress"     boolean NOT NULL DEFAULT false,
    "synced_at"    timestamp with time zone,
    "last_read_at" timestamp with time zone,
    "created_at"   timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY ("user_id", "playlist_urn")
);
CREATE INDEX "user_owned_playlists_user_created_idx"
    ON "user_owned_playlists" ("user_id", "created_at" DESC);

CREATE TABLE "user_owned_tracks" (
    "user_id"      text NOT NULL,
    "sc_track_id"  text NOT NULL,
    "payload"      jsonb,
    "progress"     boolean NOT NULL DEFAULT false,
    "synced_at"    timestamp with time zone,
    "last_read_at" timestamp with time zone,
    "created_at"   timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY ("user_id", "sc_track_id")
);
CREATE INDEX "user_owned_tracks_user_created_idx"
    ON "user_owned_tracks" ("user_id", "created_at" DESC);

-- =========================================================================
-- Shared entity caches.
-- Tracks хранятся в indexed_tracks.raw_sc_data — отдельная cached_tracks не нужна.
-- =========================================================================

CREATE TABLE "cached_users" (
    "user_urn"     text PRIMARY KEY,
    "payload"      jsonb NOT NULL,
    "synced_at"    timestamp with time zone NOT NULL,
    "last_read_at" timestamp with time zone,
    "created_at"   timestamp with time zone NOT NULL DEFAULT now()
);
CREATE INDEX "cached_users_synced_at_idx"    ON "cached_users" ("synced_at");
CREATE INDEX "cached_users_last_read_at_idx" ON "cached_users" ("last_read_at");

CREATE TABLE "cached_playlists" (
    "playlist_urn" text PRIMARY KEY,
    "payload"      jsonb NOT NULL,
    "synced_at"    timestamp with time zone NOT NULL,
    "last_read_at" timestamp with time zone,
    "created_at"   timestamp with time zone NOT NULL DEFAULT now()
);
CREATE INDEX "cached_playlists_synced_at_idx"    ON "cached_playlists" ("synced_at");
CREATE INDEX "cached_playlists_last_read_at_idx" ON "cached_playlists" ("last_read_at");

CREATE TABLE "cached_playlist_tracks" (
    "playlist_urn" text NOT NULL,
    "position"     integer NOT NULL,
    "sc_track_id"  text NOT NULL,
    PRIMARY KEY ("playlist_urn", "position")
);
CREATE INDEX "cached_playlist_tracks_track_idx" ON "cached_playlist_tracks" ("sc_track_id");
