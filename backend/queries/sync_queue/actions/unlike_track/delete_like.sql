DELETE
FROM user_likes_tracks
WHERE user_id = $1
  AND sc_track_id = $2
  AND wanted_state = false
