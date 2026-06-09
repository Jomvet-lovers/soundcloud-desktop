SELECT sc_track_id
FROM user_events
WHERE sc_user_id = ANY ($1)
  AND event_type IN ('full_play', 'play_complete')
  AND created_at > NOW() - make_interval(days => $2::int)
GROUP BY sc_track_id
ORDER BY MAX(created_at) DESC LIMIT $3
