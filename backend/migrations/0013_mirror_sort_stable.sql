-- Stable tie-breaker для ORDER BY на mirror-таблицах: батчевый refresh пишет
-- 500 строк за транзакцию с одинаковым created_at, без второго ключа в индексе
-- ORDER BY created_at DESC выдаёт строки в неопределённом порядке внутри батча.
-- CREATE INDEX держит SHARE lock (блокирует writes, не reads). Если объёмы
-- большие — на проде пре-создать руками с CONCURRENTLY под этими же именами.

CREATE INDEX IF NOT EXISTS "user_likes_tracks_user_created_key_idx"
    ON "user_likes_tracks" ("user_id", "created_at" DESC, "sc_track_id" DESC);
DROP INDEX IF EXISTS "user_likes_tracks_user_created_idx";

CREATE INDEX IF NOT EXISTS "user_likes_playlists_user_created_key_idx"
    ON "user_likes_playlists" ("user_id", "created_at" DESC, "playlist_urn" DESC);
DROP INDEX IF EXISTS "user_likes_playlists_user_created_idx";

CREATE INDEX IF NOT EXISTS "user_followings_user_created_key_idx"
    ON "user_followings" ("user_id", "created_at" DESC, "target_user_urn" DESC);
DROP INDEX IF EXISTS "user_followings_user_created_idx";

CREATE INDEX IF NOT EXISTS "user_owned_playlists_user_created_key_idx"
    ON "user_owned_playlists" ("user_id", "created_at" DESC, "playlist_urn" DESC);
DROP INDEX IF EXISTS "user_owned_playlists_user_created_idx";

CREATE INDEX IF NOT EXISTS "user_owned_tracks_user_created_key_idx"
    ON "user_owned_tracks" ("user_id", "created_at" DESC, "sc_track_id" DESC);
DROP INDEX IF EXISTS "user_owned_tracks_user_created_idx";
