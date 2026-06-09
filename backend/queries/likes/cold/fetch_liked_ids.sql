SELECT sc_track_id
FROM user_likes_tracks
WHERE user_id = ANY ($1)
  AND wanted_state = true
  AND sc_track_id = ANY ($2)
