-- Enrich pipeline: artists, albums, wanted tracks, SC accounts, socials,
-- counters, плюс расширение indexed_tracks под канонизацию и кравл.

CREATE TABLE "artists" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"name" text NOT NULL,
	"normalized_name" text NOT NULL,
	"mb_artist_id" text,
	"spotify_artist_id" text,
	"genius_artist_id" text,
	"isni" text,
	"country" varchar(8),
	"avatar_url" text,
	"bio" text,
	"sc_user_id" text,
	"source" varchar(16) NOT NULL,
	"confidence" real NOT NULL DEFAULT 0,
	"merged_into" uuid REFERENCES "artists"("id"),
	"last_crawled_at" timestamp with time zone,
	"crawl_attempts" smallint NOT NULL DEFAULT 0,
	"mb_crawl_offset" integer NOT NULL DEFAULT 0,
	"genius_crawl_offset" integer NOT NULL DEFAULT 0,
	"created_at" timestamp with time zone NOT NULL DEFAULT now(),
	"updated_at" timestamp with time zone NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX "artists_normalized_name_uq" ON "artists" ("normalized_name") WHERE merged_into IS NULL;
CREATE UNIQUE INDEX "artists_mb_uq" ON "artists" ("mb_artist_id") WHERE mb_artist_id IS NOT NULL;
CREATE UNIQUE INDEX "artists_spotify_uq" ON "artists" ("spotify_artist_id") WHERE spotify_artist_id IS NOT NULL;
CREATE UNIQUE INDEX "artists_genius_uq" ON "artists" ("genius_artist_id") WHERE genius_artist_id IS NOT NULL;
CREATE INDEX "artists_sc_user_id_idx" ON "artists" ("sc_user_id") WHERE sc_user_id IS NOT NULL;
CREATE INDEX "artists_merged_into_idx" ON "artists" ("merged_into") WHERE merged_into IS NOT NULL;
CREATE INDEX "artists_crawl_pickup_idx" ON "artists" ("last_crawled_at" NULLS FIRST) WHERE merged_into IS NULL AND confidence >= 0.5;

CREATE TABLE "albums" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"title" text NOT NULL,
	"normalized_title" text NOT NULL,
	"primary_artist_id" uuid REFERENCES "artists"("id"),
	"type" varchar(16) NOT NULL DEFAULT 'album',
	"release_year" smallint,
	"mb_release_id" text,
	"spotify_album_id" text,
	"genius_album_id" text,
	"cover_url" text,
	"source" varchar(16) NOT NULL,
	"confidence" real NOT NULL DEFAULT 0,
	"created_at" timestamp with time zone NOT NULL DEFAULT now(),
	"updated_at" timestamp with time zone NOT NULL DEFAULT now()
);
CREATE INDEX "albums_primary_artist_idx" ON "albums" ("primary_artist_id");
CREATE INDEX "albums_normalized_title_idx" ON "albums" ("normalized_title");
CREATE UNIQUE INDEX "albums_mb_uq" ON "albums" ("mb_release_id") WHERE mb_release_id IS NOT NULL;
CREATE UNIQUE INDEX "albums_spotify_uq" ON "albums" ("spotify_album_id") WHERE spotify_album_id IS NOT NULL;

CREATE TABLE "track_artists" (
	"indexed_track_id" uuid NOT NULL REFERENCES "indexed_tracks"("id") ON DELETE CASCADE,
	"artist_id" uuid NOT NULL REFERENCES "artists"("id") ON DELETE CASCADE,
	"role" varchar(16) NOT NULL,
	"position" smallint NOT NULL DEFAULT 0,
	"source" varchar(16) NOT NULL,
	"confidence" real NOT NULL DEFAULT 0,
	PRIMARY KEY ("indexed_track_id", "artist_id", "role")
);
CREATE INDEX "track_artists_artist_idx" ON "track_artists" ("artist_id");
CREATE INDEX "track_artists_role_idx" ON "track_artists" ("role");

CREATE TABLE "album_artists" (
	"album_id" uuid NOT NULL REFERENCES "albums"("id") ON DELETE CASCADE,
	"artist_id" uuid NOT NULL REFERENCES "artists"("id") ON DELETE CASCADE,
	"role" varchar(16) NOT NULL,
	PRIMARY KEY ("album_id", "artist_id", "role")
);
CREATE INDEX "album_artists_artist_idx" ON "album_artists" ("artist_id");

CREATE TABLE "album_tracks" (
	"album_id" uuid NOT NULL REFERENCES "albums"("id") ON DELETE CASCADE,
	"indexed_track_id" uuid NOT NULL REFERENCES "indexed_tracks"("id") ON DELETE CASCADE,
	"position" smallint,
	"disc_number" smallint NOT NULL DEFAULT 1,
	PRIMARY KEY ("album_id", "indexed_track_id")
);
CREATE INDEX "album_tracks_track_idx" ON "album_tracks" ("indexed_track_id");

CREATE TABLE "artist_coplay" (
	"a_id" uuid NOT NULL REFERENCES "artists"("id") ON DELETE CASCADE,
	"b_id" uuid NOT NULL REFERENCES "artists"("id") ON DELETE CASCADE,
	"weight" real NOT NULL DEFAULT 0,
	"last_seen" timestamp with time zone NOT NULL DEFAULT now(),
	PRIMARY KEY ("a_id", "b_id"),
	CHECK ("a_id" < "b_id")
);
CREATE INDEX "artist_coplay_a_weight_idx" ON "artist_coplay" ("a_id", "weight" DESC);
CREATE INDEX "artist_coplay_b_weight_idx" ON "artist_coplay" ("b_id", "weight" DESC);

CREATE TABLE "artist_socials" (
	"artist_id" uuid NOT NULL REFERENCES "artists"("id") ON DELETE CASCADE,
	"kind" varchar(32) NOT NULL,
	"url" text NOT NULL,
	"source" varchar(16) NOT NULL,
	"verified" boolean NOT NULL DEFAULT false,
	"created_at" timestamp with time zone NOT NULL DEFAULT now(),
	"updated_at" timestamp with time zone NOT NULL DEFAULT now(),
	PRIMARY KEY ("artist_id", "url")
);
CREATE INDEX "artist_socials_kind_idx" ON "artist_socials" ("kind");
CREATE INDEX "artist_socials_artist_kind_idx" ON "artist_socials" ("artist_id", "kind");

CREATE TABLE "artist_sc_accounts" (
	"artist_id" uuid NOT NULL REFERENCES "artists"("id") ON DELETE CASCADE,
	"sc_user_id" text NOT NULL,
	"role" varchar(8) NOT NULL DEFAULT 'main',
	"source" varchar(16) NOT NULL,
	"verified" boolean NOT NULL DEFAULT false,
	"notes" text,
	"created_at" timestamp with time zone NOT NULL DEFAULT now(),
	"updated_at" timestamp with time zone NOT NULL DEFAULT now(),
	PRIMARY KEY ("artist_id", "sc_user_id")
);
CREATE INDEX "artist_sc_accounts_user_idx" ON "artist_sc_accounts" ("sc_user_id");
CREATE INDEX "artist_sc_accounts_role_idx" ON "artist_sc_accounts" ("role");

CREATE TABLE "wanted_tracks" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"title" text NOT NULL,
	"normalized_title" text NOT NULL,
	"primary_artist_id" uuid REFERENCES "artists"("id") ON DELETE SET NULL,
	"isrc" text,
	"duration_ms" integer,
	"release_year" smallint,
	"source" varchar(16) NOT NULL,
	"external_id" text,
	"status" varchar(16) NOT NULL DEFAULT 'wanted',
	"indexed_track_id" uuid REFERENCES "indexed_tracks"("id") ON DELETE SET NULL,
	"discovered_at" timestamp with time zone NOT NULL DEFAULT now(),
	"updated_at" timestamp with time zone NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX "wanted_tracks_isrc_uq" ON "wanted_tracks" ("isrc") WHERE isrc IS NOT NULL;
CREATE UNIQUE INDEX "wanted_tracks_external_uq" ON "wanted_tracks" ("source", "external_id") WHERE external_id IS NOT NULL;
CREATE INDEX "wanted_tracks_artist_idx" ON "wanted_tracks" ("primary_artist_id") WHERE primary_artist_id IS NOT NULL;
CREATE INDEX "wanted_tracks_status_idx" ON "wanted_tracks" ("status");
CREATE INDEX "wanted_tracks_artist_title_idx" ON "wanted_tracks" ("primary_artist_id", "normalized_title") WHERE primary_artist_id IS NOT NULL;

CREATE TABLE "wanted_track_artists" (
	"wanted_track_id" uuid NOT NULL REFERENCES "wanted_tracks"("id") ON DELETE CASCADE,
	"artist_id" uuid NOT NULL REFERENCES "artists"("id") ON DELETE CASCADE,
	"role" varchar(16) NOT NULL,
	"position" smallint NOT NULL DEFAULT 0,
	PRIMARY KEY ("wanted_track_id", "artist_id", "role")
);
CREATE INDEX "wanted_track_artists_artist_idx" ON "wanted_track_artists" ("artist_id");
CREATE INDEX "wanted_track_artists_role_idx" ON "wanted_track_artists" ("role");

CREATE TABLE "wanted_track_albums" (
	"wanted_track_id" uuid NOT NULL REFERENCES "wanted_tracks"("id") ON DELETE CASCADE,
	"album_id" uuid NOT NULL REFERENCES "albums"("id") ON DELETE CASCADE,
	"position" smallint NOT NULL DEFAULT 0,
	PRIMARY KEY ("wanted_track_id", "album_id")
);
CREATE INDEX "wanted_track_albums_album_idx" ON "wanted_track_albums" ("album_id");

CREATE TABLE "sc_track_counters" (
	"sc_track_id" text PRIMARY KEY NOT NULL,
	"play_count" bigint,
	"likes_count" bigint,
	"reposts_count" bigint,
	"comment_count" bigint,
	"fetched_at" timestamp with time zone NOT NULL DEFAULT now()
);
CREATE INDEX "sc_track_counters_fetched_at_idx" ON "sc_track_counters" ("fetched_at");

ALTER TABLE "indexed_tracks" ADD COLUMN "canonical_track_id" uuid;
ALTER TABLE "indexed_tracks" ADD COLUMN "isrc" text;
ALTER TABLE "indexed_tracks" ADD COLUMN "primary_artist_id" uuid REFERENCES "artists"("id");
ALTER TABLE "indexed_tracks" ADD COLUMN "album_id" uuid REFERENCES "albums"("id");
ALTER TABLE "indexed_tracks" ADD COLUMN "album_position" smallint;
ALTER TABLE "indexed_tracks" ADD COLUMN "enrich_state" varchar(16) NOT NULL DEFAULT 'pending';
ALTER TABLE "indexed_tracks" ADD COLUMN "enrich_attempts" smallint NOT NULL DEFAULT 0;
ALTER TABLE "indexed_tracks" ADD COLUMN "enrich_source" varchar(16);
ALTER TABLE "indexed_tracks" ADD COLUMN "enrich_confidence" real;
ALTER TABLE "indexed_tracks" ADD COLUMN "enriched_at" timestamp with time zone;
ALTER TABLE "indexed_tracks" ADD COLUMN "refreshed_at" timestamp with time zone NOT NULL DEFAULT now();
ALTER TABLE "indexed_tracks" ADD COLUMN "upload_kind" varchar(16) NOT NULL DEFAULT 'unknown';
ALTER TABLE "indexed_tracks" ADD COLUMN "audio_fingerprint" text;
ALTER TABLE "indexed_tracks" ADD COLUMN "uploader_sc_user_id" text;
ALTER TABLE "indexed_tracks" ADD COLUMN "release_year" smallint;
ALTER TABLE "indexed_tracks" ADD COLUMN "release_date" date;
CREATE INDEX "indexed_tracks_canonical_idx" ON "indexed_tracks" ("canonical_track_id") WHERE canonical_track_id IS NOT NULL;
CREATE INDEX "indexed_tracks_isrc_idx" ON "indexed_tracks" ("isrc") WHERE isrc IS NOT NULL;
CREATE INDEX "indexed_tracks_primary_artist_idx" ON "indexed_tracks" ("primary_artist_id") WHERE primary_artist_id IS NOT NULL;
CREATE INDEX "indexed_tracks_album_idx" ON "indexed_tracks" ("album_id") WHERE album_id IS NOT NULL;
CREATE INDEX "indexed_tracks_enrich_pickup_idx" ON "indexed_tracks" ("enriched_at", "enrich_attempts") WHERE enrich_state IN ('pending', 'failed');
CREATE INDEX "indexed_tracks_upload_kind_idx" ON "indexed_tracks" ("upload_kind") WHERE upload_kind <> 'unknown';
CREATE INDEX "indexed_tracks_audio_fingerprint_prefix_idx"
    ON "indexed_tracks" USING btree (substr("audio_fingerprint", 1, 64))
    WHERE "audio_fingerprint" IS NOT NULL;
CREATE INDEX "indexed_tracks_uploader_idx"
    ON "indexed_tracks" ("uploader_sc_user_id")
    WHERE uploader_sc_user_id IS NOT NULL;
CREATE INDEX "indexed_tracks_uploader_artist_idx"
    ON "indexed_tracks" ("uploader_sc_user_id", "primary_artist_id")
    WHERE uploader_sc_user_id IS NOT NULL AND primary_artist_id IS NOT NULL;
CREATE INDEX "indexed_tracks_release_year_idx"
    ON "indexed_tracks" ("release_year")
    WHERE release_year IS NOT NULL;
