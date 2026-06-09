SELECT sc_track_id,
       (EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400.0) ::real AS "age_days!"
FROM user_likes_tracks
WHERE user_id = ANY ($1)
  AND wanted_state = true
  AND created_at > NOW() - INTERVAL '365 days'
ORDER BY created_at DESC, ctid DESC
    LIMIT $2
