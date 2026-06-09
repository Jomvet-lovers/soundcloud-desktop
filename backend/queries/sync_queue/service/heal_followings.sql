-- Реэнкюив фолловингов (key = target_user_urn).
INSERT INTO sync_queue (user_id, action_type, target_urn)
SELECT m.user_id,
       CASE WHEN m.wanted_state THEN 'follow_user' ELSE 'unfollow_user' END,
       m.target_user_urn
FROM user_followings m
WHERE m.progress = true
  AND COALESCE(m.synced_at, m.created_at) < now() - interval '15 minutes'
  AND NOT EXISTS ( SELECT 1 FROM sync_queue q
    WHERE q.user_id = m.user_id
  AND q.dead = false
  AND q.target_urn = m.target_user_urn
  AND q.action_type IN ('follow_user'
    , 'unfollow_user') )
    LIMIT 500
ON CONFLICT (user_id, action_type, target_urn) DO
UPDATE SET
    next_run_at = now(), locked_at = NULL, dead = false,
    retry_count = 0, last_error = NULL
