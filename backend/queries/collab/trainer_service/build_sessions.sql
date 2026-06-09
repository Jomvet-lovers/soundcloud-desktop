-- 90d session events for item2vec training, streamed user/time-ordered.
SELECT sc_user_id, sc_track_id, created_at, event_type
FROM user_events
WHERE created_at >= $1
  AND event_type = ANY ($2)
ORDER BY sc_user_id ASC, created_at ASC
