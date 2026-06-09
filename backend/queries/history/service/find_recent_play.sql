SELECT id
FROM listening_history
WHERE soundcloud_user_id = ANY ($1)
  AND sc_track_id = $2
  AND played_at > $3
ORDER BY played_at DESC LIMIT 1
