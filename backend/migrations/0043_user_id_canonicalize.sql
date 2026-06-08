-- Канонизация user-id → bare numeric для ВСЕХ per-user-state таблиц. Прод-факт
-- (survey 2026-06): user_likes_tracks/followings/owned_* расщеплены URN+bare
-- (likes: 1.12M URN + 294k bare), остальные state-таблицы — URN. Канон = bare
-- (совпадает с entity-колонками uploader/owner/users.sc_user_id).
--
-- ИДЕМПОТЕНТНА и защищена `~ '^soundcloud:users:[0-9]+$'` — на bare-only БД no-op.
-- Merge при PK-конфликте: для лайков wanted=bool_or (НЕ теряем лайк),
-- progress=bool_and, synced/last_read=max, created=min; counters=sum; премиум=max exp.
--
-- ПРОД: прогнать ВРУЧНУЮ до деплоя (psql -f), off-peak — likes_tracks ~1.1M строк
-- мёрджится в одной tx (лок на таблицу на пару минут). ANY-риды в коде делают
-- сплит невидимым для юзера ещё ДО бэкфилла, так что спешки нет. Тяжёлые
-- append-only таблицы (user_events/listening_history/rec_*) — отдельным
-- батч-скриптом (см. docs/likes-playlists-reliability-rollout.md), они читаются
-- через ANY и в бэкфилле не обязательны.

-- ── likes/followings (wanted_state) ──────────────────────────────────────
INSERT INTO user_likes_tracks (user_id, sc_track_id, wanted_state, progress, synced_at, last_read_at, created_at)
SELECT split_part(user_id, ':', 3), sc_track_id, wanted_state, progress, synced_at, last_read_at, created_at
FROM user_likes_tracks
WHERE user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (user_id, sc_track_id) DO
UPDATE SET
    wanted_state = user_likes_tracks.wanted_state OR EXCLUDED.wanted_state,
    progress = user_likes_tracks.progress AND EXCLUDED.progress,
    synced_at = GREATEST(COALESCE (user_likes_tracks.synced_at,'epoch'::timestamptz), COALESCE (EXCLUDED.synced_at,'epoch'::timestamptz)),
    last_read_at = GREATEST(COALESCE (user_likes_tracks.last_read_at,'epoch'::timestamptz), COALESCE (EXCLUDED.last_read_at,'epoch'::timestamptz)),
    created_at = LEAST(user_likes_tracks.created_at, EXCLUDED.created_at);
DELETE
FROM user_likes_tracks
WHERE user_id ~ '^soundcloud:users:[0-9]+$';

INSERT INTO user_likes_playlists (user_id, playlist_urn, wanted_state, progress, synced_at, last_read_at, created_at)
SELECT split_part(user_id, ':', 3), playlist_urn, wanted_state, progress, synced_at, last_read_at, created_at
FROM user_likes_playlists
WHERE user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (user_id, playlist_urn) DO
UPDATE SET
    wanted_state = user_likes_playlists.wanted_state OR EXCLUDED.wanted_state,
    progress = user_likes_playlists.progress AND EXCLUDED.progress,
    synced_at = GREATEST(COALESCE (user_likes_playlists.synced_at,'epoch'::timestamptz), COALESCE (EXCLUDED.synced_at,'epoch'::timestamptz)),
    last_read_at = GREATEST(COALESCE (user_likes_playlists.last_read_at,'epoch'::timestamptz), COALESCE (EXCLUDED.last_read_at,'epoch'::timestamptz)),
    created_at = LEAST(user_likes_playlists.created_at, EXCLUDED.created_at);
DELETE
FROM user_likes_playlists
WHERE user_id ~ '^soundcloud:users:[0-9]+$';

INSERT INTO user_followings (user_id, target_user_urn, wanted_state, progress, synced_at, last_read_at, created_at)
SELECT split_part(user_id, ':', 3), target_user_urn, wanted_state, progress, synced_at, last_read_at, created_at
FROM user_followings
WHERE user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (user_id, target_user_urn) DO
UPDATE SET
    wanted_state = user_followings.wanted_state OR EXCLUDED.wanted_state,
    progress = user_followings.progress AND EXCLUDED.progress,
    synced_at = GREATEST(COALESCE (user_followings.synced_at,'epoch'::timestamptz), COALESCE (EXCLUDED.synced_at,'epoch'::timestamptz)),
    last_read_at = GREATEST(COALESCE (user_followings.last_read_at,'epoch'::timestamptz), COALESCE (EXCLUDED.last_read_at,'epoch'::timestamptz)),
    created_at = LEAST(user_followings.created_at, EXCLUDED.created_at);
DELETE
FROM user_followings
WHERE user_id ~ '^soundcloud:users:[0-9]+$';

-- ── owned (no wanted_state) ──────────────────────────────────────────────
INSERT INTO user_owned_playlists (user_id, playlist_urn, progress, synced_at, last_read_at, created_at)
SELECT split_part(user_id, ':', 3), playlist_urn, progress, synced_at, last_read_at, created_at
FROM user_owned_playlists
WHERE user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (user_id, playlist_urn) DO
UPDATE SET
    progress = user_owned_playlists.progress AND EXCLUDED.progress,
    synced_at = GREATEST(COALESCE (user_owned_playlists.synced_at,'epoch'::timestamptz), COALESCE (EXCLUDED.synced_at,'epoch'::timestamptz)),
    last_read_at = GREATEST(COALESCE (user_owned_playlists.last_read_at,'epoch'::timestamptz), COALESCE (EXCLUDED.last_read_at,'epoch'::timestamptz)),
    created_at = LEAST(user_owned_playlists.created_at, EXCLUDED.created_at);
DELETE
FROM user_owned_playlists
WHERE user_id ~ '^soundcloud:users:[0-9]+$';

INSERT INTO user_owned_tracks (user_id, sc_track_id, progress, synced_at, last_read_at, created_at)
SELECT split_part(user_id, ':', 3), sc_track_id, progress, synced_at, last_read_at, created_at
FROM user_owned_tracks
WHERE user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (user_id, sc_track_id) DO
UPDATE SET
    progress = user_owned_tracks.progress AND EXCLUDED.progress,
    synced_at = GREATEST(COALESCE (user_owned_tracks.synced_at,'epoch'::timestamptz), COALESCE (EXCLUDED.synced_at,'epoch'::timestamptz)),
    last_read_at = GREATEST(COALESCE (user_owned_tracks.last_read_at,'epoch'::timestamptz), COALESCE (EXCLUDED.last_read_at,'epoch'::timestamptz)),
    created_at = LEAST(user_owned_tracks.created_at, EXCLUDED.created_at);
DELETE
FROM user_owned_tracks
WHERE user_id ~ '^soundcloud:users:[0-9]+$';

-- ── sync_queue (unique user_id,action_type,target_urn; in-flight не трогаем) ──
INSERT INTO sync_queue (user_id, action_type, target_urn, payload, retry_count, last_error, next_run_at, created_at,
                        dead, failed_at)
SELECT split_part(user_id, ':', 3),
       action_type,
       target_urn,
       payload,
       retry_count,
       last_error,
       next_run_at,
       created_at,
       dead,
       failed_at
FROM sync_queue
WHERE user_id ~ '^soundcloud:users:[0-9]+$' AND locked_at IS NULL
ON CONFLICT (user_id, action_type, target_urn) DO
UPDATE SET
    retry_count = LEAST(sync_queue.retry_count, EXCLUDED.retry_count),
    dead = sync_queue.dead AND EXCLUDED.dead,
    next_run_at = LEAST(sync_queue.next_run_at, EXCLUDED.next_run_at),
    created_at = LEAST(sync_queue.created_at, EXCLUDED.created_at),
    payload = COALESCE (EXCLUDED.payload, sync_queue.payload);
DELETE
FROM sync_queue
WHERE user_id ~ '^soundcloud:users:[0-9]+$' AND locked_at IS NULL;

-- ── disliked_tracks (unique sc_user_id,sc_track_id; id=auto uuid) ─────────
INSERT INTO disliked_tracks (sc_user_id, sc_track_id, track_data, created_at)
SELECT split_part(sc_user_id, ':', 3), sc_track_id, track_data, created_at
FROM disliked_tracks
WHERE sc_user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (sc_user_id, sc_track_id) DO
UPDATE SET
    track_data = COALESCE (EXCLUDED.track_data, disliked_tracks.track_data),
    created_at = LEAST(disliked_tracks.created_at, EXCLUDED.created_at);
DELETE
FROM disliked_tracks
WHERE sc_user_id ~ '^soundcloud:users:[0-9]+$';

-- ── subscriptions (PK user_urn — премиум, max exp) ───────────────────────
INSERT INTO subscriptions (user_urn, exp_date)
SELECT split_part(user_urn, ':', 3), exp_date
FROM subscriptions
WHERE user_urn ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (user_urn) DO
UPDATE SET exp_date = GREATEST(subscriptions.exp_date, EXCLUDED.exp_date);
DELETE
FROM subscriptions
WHERE user_urn ~ '^soundcloud:users:[0-9]+$';

-- ── user_auras (PK user_urn — newer updated_at wins) ─────────────────────
INSERT INTO user_auras (user_urn, aura_id, custom_hex, updated_at)
SELECT split_part(user_urn, ':', 3), aura_id, custom_hex, updated_at
FROM user_auras
WHERE user_urn ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (user_urn) DO
UPDATE SET
    aura_id = CASE WHEN EXCLUDED.updated_at > user_auras.updated_at THEN EXCLUDED.aura_id ELSE user_auras.aura_id END,
    custom_hex = CASE WHEN EXCLUDED.updated_at > user_auras.updated_at THEN EXCLUDED.custom_hex ELSE user_auras.custom_hex
END,
  updated_at = GREATEST(user_auras.updated_at, EXCLUDED.updated_at);
DELETE
FROM user_auras
WHERE user_urn ~ '^soundcloud:users:[0-9]+$';

-- ── user_profiles (PK soundcloud_user_id — newer synced_at wins) ─────────
-- Замечание: login пишет профиль в URN-форме; после бэкфилла новые логины снова
-- создадут URN-строку, но get_profile_cold читает ANY+LIMIT 1 по свежести —
-- функционально ок. Полный канон профилей — когда login начнёт писать bare.
INSERT INTO user_profiles (soundcloud_user_id, profile_json, synced_at)
SELECT split_part(soundcloud_user_id, ':', 3), profile_json, synced_at
FROM user_profiles
WHERE soundcloud_user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (soundcloud_user_id) DO
UPDATE SET
    profile_json = CASE WHEN EXCLUDED.synced_at > user_profiles.synced_at THEN EXCLUDED.profile_json ELSE user_profiles.profile_json END,
    synced_at = GREATEST(user_profiles.synced_at, EXCLUDED.synced_at);
DELETE
FROM user_profiles
WHERE soundcloud_user_id ~ '^soundcloud:users:[0-9]+$';

-- ── cluster_bandit_stats (PK sc_user_id,cluster_id — суммируем счётчики) ──
INSERT INTO cluster_bandit_stats (sc_user_id, cluster_id, shows, clicks, completes, updated_at)
SELECT split_part(sc_user_id, ':', 3), cluster_id, shows, clicks, completes, updated_at
FROM cluster_bandit_stats
WHERE sc_user_id ~ '^soundcloud:users:[0-9]+$'
ON CONFLICT (sc_user_id, cluster_id) DO
UPDATE SET
    shows = cluster_bandit_stats.shows + EXCLUDED.shows,
    clicks = cluster_bandit_stats.clicks + EXCLUDED.clicks,
    completes = cluster_bandit_stats.completes + EXCLUDED.completes,
    updated_at = GREATEST(cluster_bandit_stats.updated_at, EXCLUDED.updated_at);
DELETE
FROM cluster_bandit_stats
WHERE sc_user_id ~ '^soundcloud:users:[0-9]+$';
