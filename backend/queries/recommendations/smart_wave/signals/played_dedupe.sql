SELECT DISTINCT sc_track_id
FROM user_events
WHERE sc_user_id = ANY ($1)
  AND created_at > NOW() - INTERVAL '180 days'
ORDER BY sc_track_id
    LIMIT $2
