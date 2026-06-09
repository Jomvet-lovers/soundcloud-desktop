SELECT sc_track_id
FROM user_events
WHERE sc_user_id = ANY ($1)
  AND event_type IN ('full_play', 'like', 'playlist_add')
  AND created_at > NOW() - make_interval(hours = > $2::int)
ORDER BY created_at DESC LIMIT $3
