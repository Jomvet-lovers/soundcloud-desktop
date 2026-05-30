-- Discover: artists.popularity_score — гибрид внешних SC play_count и наших
-- внутренних full_play. Каталог артистов раньше сортировал trending_score,
-- который на молодой базе вырождался в алфавит (мусорные 1-трековые артисты
-- наверху). Считается в discover.refresh_aggregates, LN-нормализация как у
-- albums.popularity_score.
--
-- Большой индекс на проде создаём CONCURRENTLY заранее — миграция тогда no-op.

ALTER TABLE "artists"
    ADD COLUMN IF NOT EXISTS "popularity_score" real NOT NULL DEFAULT 0;

-- Бэкфилл прямо в миграции: иначе на первом деплое весь каталог имеет
-- popularity_score=0 до первого refresh_aggregates, и новый дефолтный сорт
-- 'popular' вырождается в алфавит (тот самый мусор). Вес 10000 = INTERNAL_PLAY_WEIGHT.
-- max_parallel_workers_per_gather=0: у прод-инстанса узкий /dev/shm под parallel workers.
SET LOCAL max_parallel_workers_per_gather = 0;

WITH sc AS (SELECT t.primary_artist_id AS artist_id, SUM(c.play_count) ::bigint AS plays
            FROM sc_track_counters c
                     JOIN tracks t ON t.sc_track_id = c.sc_track_id
            WHERE t.primary_artist_id IS NOT NULL
            GROUP BY t.primary_artist_id),
     internal AS (SELECT t.primary_artist_id AS artist_id, COUNT(*) ::bigint AS fp
                  FROM user_events ue
                           JOIN tracks t ON t.sc_track_id = ue.sc_track_id
                  WHERE ue.event_type = 'full_play'
                    AND t.primary_artist_id IS NOT NULL
                  GROUP BY t.primary_artist_id),
     combined AS (SELECT COALESCE(sc.artist_id, internal.artist_id) AS artist_id,
                         COALESCE(sc.plays, 0) + COALESCE(internal.fp, 0) * 10000::bigint AS score
                  FROM sc
                           FULL OUTER JOIN internal ON sc.artist_id = internal.artist_id),
     denom AS (SELECT GREATEST(MAX(score), 1) ::bigint AS m FROM combined)
UPDATE artists a
SET popularity_score = LEAST(
        1.0::real,
        (LN(GREATEST(cm.score, 0) + 1)::real / NULLIF(LN((SELECT m FROM denom) + 1)::real, 0))
                       ) FROM combined cm
WHERE a.id = cm.artist_id AND cm.score > 0 AND a.merged_into IS NULL;

CREATE INDEX IF NOT EXISTS "artists_discover_popular_idx"
    ON "artists" ("popularity_score" DESC, "normalized_name", "id")
    WHERE merged_into IS NULL
    AND (track_count_primary > 0 OR track_count_featured > 0);
