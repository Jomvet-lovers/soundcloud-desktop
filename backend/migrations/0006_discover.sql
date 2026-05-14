-- Discover: денормализованные агрегаты для каталога артистов/альбомов.
-- Обновляются периодической задачей `discover.refresh_aggregates`,
-- индексы заточены под keyset-пагинацию.

ALTER TABLE "artists" ADD COLUMN "monthly_listeners"     bigint  NOT NULL DEFAULT 0;
ALTER TABLE "artists" ADD COLUMN "trending_score"        real    NOT NULL DEFAULT 0;
ALTER TABLE "artists" ADD COLUMN "tags"                  text[]  NOT NULL DEFAULT '{}'::text[];
ALTER TABLE "artists" ADD COLUMN "is_star"               boolean NOT NULL DEFAULT false;
ALTER TABLE "artists" ADD COLUMN "star_aura_id"          text;
ALTER TABLE "artists" ADD COLUMN "star_custom_hex"       varchar(7);
ALTER TABLE "artists" ADD COLUMN "track_count_primary"   integer NOT NULL DEFAULT 0;
ALTER TABLE "artists" ADD COLUMN "track_count_featured"  integer NOT NULL DEFAULT 0;
ALTER TABLE "artists" ADD COLUMN "album_count_denorm"    integer NOT NULL DEFAULT 0;
ALTER TABLE "artists" ADD COLUMN "aggregates_updated_at" timestamp with time zone;

ALTER TABLE "albums" ADD COLUMN "popularity_score"  real    NOT NULL DEFAULT 0;
ALTER TABLE "albums" ADD COLUMN "release_date"      date;
ALTER TABLE "albums" ADD COLUMN "track_count"       integer NOT NULL DEFAULT 0;
ALTER TABLE "albums" ADD COLUMN "total_duration_ms" bigint  NOT NULL DEFAULT 0;
ALTER TABLE "albums" ADD COLUMN "is_star_artist"    boolean NOT NULL DEFAULT false;
ALTER TABLE "albums" ADD COLUMN "aggregates_updated_at" timestamp with time zone;

CREATE INDEX "artists_discover_trending_idx"
    ON "artists" ("trending_score" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL;

CREATE INDEX "artists_discover_listeners_idx"
    ON "artists" ("monthly_listeners" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL;

CREATE INDEX "artists_discover_tracks_idx"
    ON "artists" ("track_count_primary" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL;

CREATE INDEX "artists_discover_star_idx"
    ON "artists" ("is_star" DESC, "trending_score" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL;

CREATE INDEX "artists_discover_az_idx"
    ON "artists" ("normalized_name", "id")
    WHERE merged_into IS NULL;

CREATE INDEX "artists_tags_gin"
    ON "artists" USING GIN ("tags");

CREATE INDEX "albums_discover_recent_idx"
    ON "albums" ("release_date" DESC NULLS LAST, "release_year" DESC NULLS LAST, "normalized_title", "id");

CREATE INDEX "albums_discover_popular_idx"
    ON "albums" ("popularity_score" DESC, "normalized_title", "id");

CREATE INDEX "albums_discover_tracks_idx"
    ON "albums" ("track_count" DESC, "normalized_title", "id");

CREATE INDEX "albums_discover_az_idx"
    ON "albums" ("normalized_title", "id");

CREATE INDEX "albums_discover_kind_idx"
    ON "albums" ("type");

CREATE INDEX "albums_discover_fresh_idx"
    ON "albums" ("release_date" DESC) WHERE release_date IS NOT NULL;

CREATE INDEX "albums_discover_star_recent_idx"
    ON "albums" ("is_star_artist", "release_date" DESC NULLS LAST, "track_count" DESC)
    WHERE track_count >= 4;