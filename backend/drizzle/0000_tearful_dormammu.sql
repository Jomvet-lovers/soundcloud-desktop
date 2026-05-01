CREATE TABLE "cdn_tracks" (
	"id" uuid PRIMARY KEY NOT NULL,
	"track_urn" text NOT NULL,
	"quality" varchar(4) NOT NULL,
	"cdn_path" text,
	"status" varchar(16) DEFAULT 'pending' NOT NULL,
	"last_accessed_at" timestamp with time zone DEFAULT NOW(),
	"created_at" timestamp DEFAULT now() NOT NULL,
	"updated_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "disliked_tracks" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"sc_user_id" text NOT NULL,
	"sc_track_id" text NOT NULL,
	"track_data" jsonb,
	"created_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "featured_items" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"type" varchar(20) NOT NULL,
	"sc_urn" text NOT NULL,
	"weight" integer DEFAULT 1 NOT NULL,
	"active" boolean DEFAULT true NOT NULL,
	"created_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "indexed_tracks" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"sc_track_id" text NOT NULL,
	"title" text,
	"genre" text,
	"tags" text[],
	"duration_ms" integer,
	"artwork_url" text,
	"stream_url" text,
	"raw_sc_data" jsonb,
	"indexed_at" timestamp with time zone,
	"language" varchar(8),
	"language_confidence" real,
	"s3_verified_at" timestamp with time zone,
	"s3_missing_at" timestamp with time zone,
	"created_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "link_requests" (
	"id" uuid PRIMARY KEY NOT NULL,
	"claim_token" text NOT NULL,
	"mode" varchar(8) NOT NULL,
	"source_session_id" uuid,
	"target_session_id" uuid,
	"status" varchar(16) DEFAULT 'pending' NOT NULL,
	"error" text,
	"created_at" timestamp DEFAULT now() NOT NULL,
	"expires_at" timestamp NOT NULL
);
--> statement-breakpoint
CREATE TABLE "listening_history" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"soundcloud_user_id" text NOT NULL,
	"sc_track_id" text NOT NULL,
	"title" text NOT NULL,
	"artist_name" text NOT NULL,
	"artist_urn" text,
	"artwork_url" text,
	"duration" integer NOT NULL,
	"played_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "local_likes" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"soundcloud_user_id" text NOT NULL,
	"sc_track_id" text NOT NULL,
	"track_data" jsonb NOT NULL,
	"created_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "login_requests" (
	"id" uuid PRIMARY KEY NOT NULL,
	"state" text NOT NULL,
	"code_verifier" text NOT NULL,
	"oauth_app_id" text,
	"target_session_id" uuid,
	"status" varchar(16) DEFAULT 'pending' NOT NULL,
	"result_session_id" uuid,
	"error" text,
	"created_at" timestamp DEFAULT now() NOT NULL,
	"expires_at" timestamp NOT NULL
);
--> statement-breakpoint
CREATE TABLE "lyrics_cache" (
	"sc_track_id" text PRIMARY KEY NOT NULL,
	"synced_lrc" text,
	"plain_text" text,
	"source" varchar(16) NOT NULL,
	"language" varchar(8),
	"language_confidence" real,
	"embedded_at" timestamp with time zone,
	"created_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "oauth_apps" (
	"id" uuid PRIMARY KEY NOT NULL,
	"name" text NOT NULL,
	"client_id" text NOT NULL,
	"client_secret" text NOT NULL,
	"redirect_uri" text NOT NULL,
	"active" boolean DEFAULT true NOT NULL,
	"last_used_at" timestamp with time zone,
	"created_at" timestamp DEFAULT now() NOT NULL,
	"updated_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "pending_actions" (
	"id" uuid PRIMARY KEY NOT NULL,
	"session_id" text NOT NULL,
	"action_type" varchar(32) NOT NULL,
	"target_urn" text NOT NULL,
	"payload" jsonb,
	"status" varchar(16) DEFAULT 'pending' NOT NULL,
	"error" text,
	"retry_count" integer DEFAULT 0 NOT NULL,
	"created_at" timestamp DEFAULT now() NOT NULL,
	"updated_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "sessions" (
	"id" uuid PRIMARY KEY NOT NULL,
	"access_token" text NOT NULL,
	"refresh_token" text NOT NULL,
	"expires_at" timestamp NOT NULL,
	"scope" text NOT NULL,
	"soundcloud_user_id" text,
	"username" text,
	"oauth_app_id" text,
	"created_at" timestamp DEFAULT now() NOT NULL,
	"updated_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "subscriptions" (
	"user_urn" text PRIMARY KEY NOT NULL,
	"exp_date" bigint NOT NULL
);
--> statement-breakpoint
CREATE TABLE "user_events" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"sc_user_id" text NOT NULL,
	"sc_track_id" text NOT NULL,
	"event_type" text NOT NULL,
	"weight" double precision NOT NULL,
	"seeded" boolean DEFAULT false NOT NULL,
	"taste_applied_at" timestamp with time zone,
	"created_at" timestamp DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE INDEX "cdn_tracks_track_urn_idx" ON "cdn_tracks" USING btree ("track_urn");--> statement-breakpoint
CREATE INDEX "cdn_tracks_status_idx" ON "cdn_tracks" USING btree ("status");--> statement-breakpoint
CREATE INDEX "cdn_tracks_last_accessed_idx" ON "cdn_tracks" USING btree ("last_accessed_at");--> statement-breakpoint
CREATE UNIQUE INDEX "cdn_tracks_urn_quality_uq" ON "cdn_tracks" USING btree ("track_urn","quality");--> statement-breakpoint
CREATE INDEX "disliked_tracks_sc_user_id_idx" ON "disliked_tracks" USING btree ("sc_user_id");--> statement-breakpoint
CREATE INDEX "disliked_tracks_sc_track_id_idx" ON "disliked_tracks" USING btree ("sc_track_id");--> statement-breakpoint
CREATE UNIQUE INDEX "disliked_tracks_user_track_uq" ON "disliked_tracks" USING btree ("sc_user_id","sc_track_id");--> statement-breakpoint
CREATE UNIQUE INDEX "indexed_tracks_sc_track_id_uq" ON "indexed_tracks" USING btree ("sc_track_id");--> statement-breakpoint
CREATE INDEX "indexed_tracks_language_idx" ON "indexed_tracks" USING btree ("language");--> statement-breakpoint
CREATE INDEX "indexed_tracks_s3_verified_at_idx" ON "indexed_tracks" USING btree ("s3_verified_at");--> statement-breakpoint
CREATE INDEX "indexed_tracks_s3_missing_at_idx" ON "indexed_tracks" USING btree ("s3_missing_at");--> statement-breakpoint
CREATE UNIQUE INDEX "link_requests_claim_token_uq" ON "link_requests" USING btree ("claim_token");--> statement-breakpoint
CREATE INDEX "listening_history_user_id_idx" ON "listening_history" USING btree ("soundcloud_user_id");--> statement-breakpoint
CREATE INDEX "local_likes_user_id_idx" ON "local_likes" USING btree ("soundcloud_user_id");--> statement-breakpoint
CREATE UNIQUE INDEX "local_likes_user_track_uq" ON "local_likes" USING btree ("soundcloud_user_id","sc_track_id");--> statement-breakpoint
CREATE UNIQUE INDEX "login_requests_state_uq" ON "login_requests" USING btree ("state");--> statement-breakpoint
CREATE INDEX "lyrics_cache_language_idx" ON "lyrics_cache" USING btree ("language");--> statement-breakpoint
CREATE INDEX "pending_actions_session_id_idx" ON "pending_actions" USING btree ("session_id");--> statement-breakpoint
CREATE INDEX "user_events_sc_user_id_idx" ON "user_events" USING btree ("sc_user_id");--> statement-breakpoint
CREATE INDEX "user_events_taste_applied_at_idx" ON "user_events" USING btree ("taste_applied_at");