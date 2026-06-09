SELECT sc_track_id,
       weight,
       (EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400.0) ::real AS "age_days!"
FROM user_events
WHERE sc_user_id = ANY ($1)
  AND event_type IN ('like', 'playlist_add')
  AND created_at > NOW() - INTERVAL '365 days'
ORDER BY created_at DESC
    LIMIT $2
