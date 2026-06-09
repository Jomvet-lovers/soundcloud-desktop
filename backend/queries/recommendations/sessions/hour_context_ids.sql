SELECT sc_track_id
FROM user_events
WHERE sc_user_id = ANY ($1)
  AND event_type IN ('full_play', 'like', 'playlist_add')
  AND created_at > NOW() - make_interval(weeks => $2::int)
  AND ABS(EXTRACT(HOUR FROM created_at)::int - $3) <= $4
  AND EXTRACT(DOW FROM created_at)::int = $5
ORDER BY created_at DESC
    LIMIT 60
