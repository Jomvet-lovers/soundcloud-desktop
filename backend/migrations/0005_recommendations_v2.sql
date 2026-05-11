ALTER TABLE "user_events"
    ADD COLUMN IF NOT EXISTS "position_pct" real;
CREATE INDEX IF NOT EXISTS "user_events_user_type_created_idx"
    ON "user_events" ("sc_user_id", "event_type", "created_at" DESC);

CREATE TABLE IF NOT EXISTS "rec_impressions" (
    "id" bigserial PRIMARY KEY,
    "sc_user_id" text NOT NULL,
    "sc_track_id" text NOT NULL,
    "cluster_id" text NOT NULL,
    "source" varchar(16) NOT NULL,
    "position" smallint NOT NULL,
    "score" real,
    "features" jsonb,
    "shown_at" timestamp with time zone NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS "rec_impressions_user_shown_idx"
    ON "rec_impressions" ("sc_user_id", "shown_at" DESC);
CREATE INDEX IF NOT EXISTS "rec_impressions_track_idx"
    ON "rec_impressions" ("sc_track_id");
CREATE INDEX IF NOT EXISTS "rec_impressions_cluster_user_idx"
    ON "rec_impressions" ("sc_user_id", "cluster_id", "shown_at" DESC);

CREATE TABLE IF NOT EXISTS "rec_hard_negatives" (
    "id" bigserial PRIMARY KEY,
    "sc_user_id" text NOT NULL,
    "sc_track_id" text NOT NULL,
    "predicted_score" real NOT NULL,
    "position_pct" real NOT NULL,
    "detected_at" timestamp with time zone NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS "rec_hard_negatives_user_idx"
    ON "rec_hard_negatives" ("sc_user_id", "detected_at" DESC);

CREATE TABLE IF NOT EXISTS "cluster_bandit_stats" (
    "sc_user_id" text NOT NULL,
    "cluster_id" text NOT NULL,
    "shows" bigint NOT NULL DEFAULT 0,
    "clicks" bigint NOT NULL DEFAULT 0,
    "completes" bigint NOT NULL DEFAULT 0,
    "updated_at" timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY ("sc_user_id", "cluster_id")
);

ALTER TABLE "indexed_tracks"
    ADD COLUMN IF NOT EXISTS "quality_score" real;
CREATE INDEX IF NOT EXISTS "indexed_tracks_quality_score_idx"
    ON "indexed_tracks" ("quality_score")
    WHERE quality_score IS NOT NULL;
CREATE INDEX IF NOT EXISTS "indexed_tracks_quality_pending_idx"
    ON "indexed_tracks" ("indexed_at")
    WHERE quality_score IS NULL AND indexed_at IS NOT NULL;
