-- Discover: артист считается «реальным», только если у него есть треки
-- (primary или featured). Альбомы без треков были основанием попасть в выдачу
-- через album_count_denorm > 0 — убираем эту лазейку.

DROP INDEX IF EXISTS "artists_discover_trending_idx";
DROP INDEX IF EXISTS "artists_discover_listeners_idx";
DROP INDEX IF EXISTS "artists_discover_tracks_idx";
DROP INDEX IF EXISTS "artists_discover_star_idx";
DROP INDEX IF EXISTS "artists_discover_az_idx";

CREATE INDEX "artists_discover_trending_idx"
    ON "artists" ("trending_score" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0 OR track_count_featured > 0);

CREATE INDEX "artists_discover_listeners_idx"
    ON "artists" ("monthly_listeners" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0 OR track_count_featured > 0);

CREATE INDEX "artists_discover_tracks_idx"
    ON "artists" ("track_count_primary" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0 OR track_count_featured > 0);

CREATE INDEX "artists_discover_star_idx"
    ON "artists" ("is_star" DESC, "trending_score" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0 OR track_count_featured > 0);

CREATE INDEX "artists_discover_az_idx"
    ON "artists" ("normalized_name", "id")
    WHERE merged_into IS NULL
      AND (track_count_primary > 0 OR track_count_featured > 0);
