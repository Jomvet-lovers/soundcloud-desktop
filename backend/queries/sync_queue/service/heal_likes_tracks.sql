-- Реэнкюив лайков треков (bare sc_track_id → urn).
INSERT INTO sync_queue (user_id, action_type, target_urn)
SELECT m.user_id,
       CASE WHEN m.wanted_state THEN 'like_track' ELSE 'unlike_track' END,
       'soundcloud:tracks:' || m.sc_track_id
FROM user_likes_tracks m
WHERE m.progress = true
  AND COALESCE(m.synced_at, m.created_at) < now() - interval '15 minutes'
  AND NOT EXISTS ( SELECT 1 FROM sync_queue q
    WHERE q.user_id = m.user_id
  AND q.dead = false
  AND q.target_urn = 'soundcloud:tracks:' || m.sc_track_id
  AND q.action_type IN ('like_track'
    , 'unlike_track') )
    LIMIT 500
ON CONFLICT (user_id, action_type, target_urn) DO
UPDATE SET
    next_run_at = now(), locked_at = NULL, dead = false,
    retry_count = 0, last_error = NULL
