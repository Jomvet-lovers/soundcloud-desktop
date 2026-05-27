-- /search/db/tracks и /search/db/playlists фильтруют через
-- (title_normalized LIKE OR LOWER(title) LIKE OR LOWER(<author>) LIKE).
-- 0022 покрыла title_normalized и LOWER(uploader_username)/LOWER(owner_username)
-- trgm-индексами, но LOWER(title) осталась без индекса — третья OR-ветка
-- ломала BitmapOr-план, поэтому подстрочный поиск ходил seq scan'ом и попадал
-- в slow-statement warn'ы.
--
-- Прод-замечание: GIN-build держит SHARE lock на таблицу. На больших объёмах
-- безопаснее пре-создать руками с CONCURRENTLY до раскатки релиза:
--   CREATE INDEX CONCURRENTLY tracks_search_title_lower_trgm
--     ON tracks USING GIN (LOWER(title) gin_trgm_ops) WHERE sharing = 'public';
-- IF NOT EXISTS внизу превратит migrate-step в no-op.

CREATE INDEX IF NOT EXISTS "tracks_search_title_lower_trgm"
    ON "tracks" USING GIN (LOWER("title") gin_trgm_ops)
    WHERE sharing = 'public';

CREATE INDEX IF NOT EXISTS "playlists_search_title_lower_trgm"
    ON "playlists" USING GIN (LOWER("title") gin_trgm_ops)
    WHERE sharing = 'public';
