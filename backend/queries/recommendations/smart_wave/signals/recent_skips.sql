SELECT sc_track_id
FROM user_events
WHERE sc_user_id = ANY ($1)
  AND event_type = 'skip'
  AND created_at > NOW() - make_interval(days => $2::int)
ORDER BY created_at DESC LIMIT $3
