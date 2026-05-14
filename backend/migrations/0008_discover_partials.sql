-- Discover-каталог фильтрует пустые сущности (альбомы без треков, артистов без
-- треков и без альбомов). Делаем индексы партишн под этот фильтр — на млн+
-- строк партишн-индекс читается на порядок быстрее full + filter.

DROP INDEX IF EXISTS "albums_discover_recent_idx";
DROP INDEX IF EXISTS "albums_discover_popular_idx";
DROP INDEX IF EXISTS "albums_discover_tracks_idx";
DROP INDEX IF EXISTS "albums_discover_az_idx";
DROP INDEX IF EXISTS "albums_discover_kind_idx";

CREATE INDEX "albums_discover_recent_idx"
    ON "albums" ("release_date" DESC NULLS LAST, "release_year" DESC NULLS LAST,
                 "normalized_title", "id")
    WHERE track_count > 0;

CREATE INDEX "albums_discover_popular_idx"
    ON "albums" ("popularity_score" DESC, "normalized_title", "id")
    WHERE track_count > 0;

CREATE INDEX "albums_discover_tracks_idx"
    ON "albums" ("track_count" DESC, "normalized_title", "id")
    WHERE track_count > 0;

CREATE INDEX "albums_discover_az_idx"
    ON "albums" ("normalized_title", "id")
    WHERE track_count > 0;

CREATE INDEX "albums_discover_kind_idx"
    ON "albums" ("type")
    WHERE track_count > 0;

CREATE INDEX "albums_discover_year_popular_idx"
    ON "albums" ("release_year" DESC, "popularity_score" DESC, "normalized_title", "id")
    WHERE track_count > 0 AND release_year IS NOT NULL;

DROP INDEX IF EXISTS "artists_discover_trending_idx";
DROP INDEX IF EXISTS "artists_discover_listeners_idx";
DROP INDEX IF EXISTS "artists_discover_tracks_idx";
DROP INDEX IF EXISTS "artists_discover_star_idx";
DROP INDEX IF EXISTS "artists_discover_az_idx";

CREATE INDEX "artists_discover_trending_idx"
    ON "artists" ("trending_score" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0
        OR track_count_featured > 0
        OR album_count_denorm > 0);

CREATE INDEX "artists_discover_listeners_idx"
    ON "artists" ("monthly_listeners" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0
        OR track_count_featured > 0
        OR album_count_denorm > 0);

CREATE INDEX "artists_discover_tracks_idx"
    ON "artists" ("track_count_primary" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0
        OR track_count_featured > 0
        OR album_count_denorm > 0);

CREATE INDEX "artists_discover_star_idx"
    ON "artists" ("is_star" DESC, "trending_score" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0
        OR track_count_featured > 0
        OR album_count_denorm > 0);

CREATE INDEX "artists_discover_az_idx"
    ON "artists" ("normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0
        OR track_count_featured > 0
        OR album_count_denorm > 0);
