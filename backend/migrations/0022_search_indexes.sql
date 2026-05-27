-- Trigram-индексы под подстрочный поиск в /search/db/*. Без них ILIKE '%q%'
-- по `tracks.title_normalized` / `users.username_normalized` /
-- `playlists.title_normalized` ходил бы full scan на млн+ строк, что под
-- горячим прод-трафиком положит pgr/CPU.
--
-- `pg_trgm` уже включён в 0007_discover_search для artists/albums.
--
-- ПРОД: GIN-build на больших таблицах держит SHARE lock и блокирует writes.
-- Если объёмы крупные — пре-создать руками с CONCURRENTLY под этими же
-- именами до раскатки релиза. Migrate-pass потом сделает IF NOT EXISTS no-op.

CREATE INDEX IF NOT EXISTS "tracks_search_title_norm_trgm"
    ON "tracks" USING GIN ("title_normalized" gin_trgm_ops)
    WHERE sharing = 'public';

CREATE INDEX IF NOT EXISTS "tracks_search_uploader_username_trgm"
    ON "tracks" USING GIN (LOWER("uploader_username") gin_trgm_ops)
    WHERE sharing = 'public' AND uploader_username IS NOT NULL;

CREATE INDEX IF NOT EXISTS "users_search_username_norm_trgm"
    ON "users" USING GIN ("username_normalized" gin_trgm_ops);

CREATE INDEX IF NOT EXISTS "users_search_username_lower_trgm"
    ON "users" USING GIN (LOWER("username") gin_trgm_ops);

CREATE INDEX IF NOT EXISTS "playlists_search_title_norm_trgm"
    ON "playlists" USING GIN ("title_normalized" gin_trgm_ops)
    WHERE sharing = 'public';

CREATE INDEX IF NOT EXISTS "playlists_search_owner_username_trgm"
    ON "playlists" USING GIN (LOWER("owner_username") gin_trgm_ops)
    WHERE sharing = 'public' AND owner_username IS NOT NULL;

-- Per-user content scan: ORDER BY play_count_sc DESC LIMIT с фильтром
-- uploader_sc_user_id уже покрыт `tracks_uploader_idx`, но без популярности.
-- Этот partial покрывает ORDER BY popularity в скоупе одного uploader'а
-- (inline-поиск на UserPage).
CREATE INDEX IF NOT EXISTS "tracks_uploader_popular_idx"
    ON "tracks" ("uploader_sc_user_id", "play_count_sc" DESC NULLS LAST, "id" DESC)
    WHERE uploader_sc_user_id IS NOT NULL AND sharing = 'public';

CREATE INDEX IF NOT EXISTS "playlists_owner_popular_idx"
    ON "playlists" ("owner_sc_user_id", "likes_count_sc" DESC NULLS LAST, "urn" DESC)
    WHERE owner_sc_user_id IS NOT NULL AND sharing = 'public';
