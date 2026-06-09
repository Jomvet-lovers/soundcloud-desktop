-- Реэнкюив лайков плейлистов (key = playlist_urn).
INSERT INTO sync_queue (user_id, action_type, target_urn)
SELECT m.user_id,
       CASE WHEN m.wanted_state THEN 'like_playlist' ELSE 'unlike_playlist' END,
       m.playlist_urn
FROM user_likes_playlists m
WHERE m.progress = true
  AND COALESCE(m.synced_at, m.created_at) < now() - interval '15 minutes'
  AND NOT EXISTS ( SELECT 1 FROM sync_queue q
    WHERE q.user_id = m.user_id
  AND q.dead = false
  AND q.target_urn = m.playlist_urn
  AND q.action_type IN ('like_playlist'
    , 'unlike_playlist') )
    LIMIT 500
ON CONFLICT (user_id, action_type, target_urn) DO
UPDATE SET
    next_run_at = now(), locked_at = NULL, dead = false,
    retry_count = 0, last_error = NULL
