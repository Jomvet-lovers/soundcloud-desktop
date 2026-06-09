SELECT sc_track_id,
       event_type,
       weight,
       position_pct,
       (EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400.0) ::real AS "age_days!"
FROM user_events
WHERE sc_user_id = ANY ($1)
  AND event_type = ANY ($2)
  AND created_at > NOW() - INTERVAL '180 days'
ORDER BY created_at DESC
    LIMIT $3
