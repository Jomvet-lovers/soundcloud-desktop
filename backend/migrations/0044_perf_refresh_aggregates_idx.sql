-- 0044: perf-индексы под горячий путь. Источник — прод pg_stat_statements
-- (top по mean/total): почти весь топ = крон discover.refresh_aggregates,
-- полно-табличные GROUP BY по track_artists / tracks / user_events. На /gather
-- отключён parallel (узкий /dev/shm, см. 0030) → один поток → ставка на index-only.
--
-- COPY ... TO stdout в pg_stat — это ночной pg_dump (бэкап), НЕ хот-путь, индексами
-- не лечится — игнор.
--
-- Большие индексы ПРЕД-СОЗДАЮТСЯ CONCURRENTLY на проде руками ДО выкатки → миграция
-- тогда no-op и migrate() на старте не берёт долгих локов. На свежей БД (пусто) —
-- обычный build мгновенно. Дропы на проде — тоже CONCURRENTLY руками заранее.
--
-- ┌─ OPS: прогнать на проде ПЕРЕД деплоем (psql, по одному стейтменту) ───────────┐
-- │ CREATE INDEX CONCURRENTLY IF NOT EXISTS track_artists_role_artist_track_idx     │
-- │     ON track_artists (role, artist_id, track_id);                               │
-- │ CREATE INDEX CONCURRENTLY IF NOT EXISTS tracks_primary_artist_genre_idx         │
-- │     ON tracks (primary_artist_id, genre)                                         │
-- │     WHERE primary_artist_id IS NOT NULL AND genre IS NOT NULL;                   │
-- │ CREATE INDEX CONCURRENTLY IF NOT EXISTS user_events_created_type_cover_idx       │
-- │     ON user_events (created_at, event_type) INCLUDE (sc_user_id, sc_track_id);   │
-- │ DROP INDEX CONCURRENTLY IF EXISTS track_artists_role_idx;                        │
-- │ DROP INDEX CONCURRENTLY IF EXISTS user_events_sc_user_id_idx;                    │
-- │ DROP INDEX CONCURRENTLY IF EXISTS user_events_created_at_idx;                    │
-- │ DROP INDEX CONCURRENTLY IF EXISTS tracks_uploader_idx;                           │
-- └─────────────────────────────────────────────────────────────────────────────────┘

-- #9 (545s/17): COUNT(*) WHERE role=$1 GROUP BY artist_id + COUNT(DISTINCT track_id)
-- WHERE role IN(...) GROUP BY artist_id. Сейчас только отдельные (role) и (artist_id)
-- → скан+сорт. Составной (role,artist_id,track_id) делает оба index-only.
CREATE INDEX IF NOT EXISTS track_artists_role_artist_track_idx
    ON track_artists (role, artist_id, track_id);
-- (role)-only теперь префикс выше — избыточен (write-амплификация на 3.4М строк).
DROP INDEX IF EXISTS track_artists_role_idx;

-- #23 (190s/17): GROUP BY primary_artist_id, LOWER(TRIM(genre)) по tracks (6.6М).
-- Покрывающий partial → index-only, partial держит компактным.
CREATE INDEX IF NOT EXISTS tracks_primary_artist_genre_idx
    ON tracks (primary_artist_id, genre)
    WHERE primary_artist_id IS NOT NULL AND genre IS NOT NULL;

-- #25 (336s/61, 2М строк/вызов) + #22/#21/#14-internal: фильтр created_at-range +
-- event_type, дальше JOIN tracks по sc_track_id. Покрывающий → index-only (снимает
-- 2М heap-fetch/вызов). NB: #25 ещё ORDER BY (sc_user_id,created_at) → сорт остаётся;
-- глубокий фикс — читать инкрементально (только новые события), это уже на app-слое.
CREATE INDEX IF NOT EXISTS user_events_created_type_cover_idx
    ON user_events (created_at, event_type) INCLUDE (sc_user_id, sc_track_id);
-- (sc_user_id)-only = префикс существующего (sc_user_id,event_type,created_at);
-- (created_at)-only = префикс нового выше. Оба избыточны.
DROP INDEX IF EXISTS user_events_sc_user_id_idx;
DROP INDEX IF EXISTS user_events_created_at_idx;

-- (uploader_sc_user_id)-only = префикс tracks_uploader_artist_idx
-- (uploader_sc_user_id, primary_artist_id) — избыточен.
DROP INDEX IF EXISTS tracks_uploader_idx;
