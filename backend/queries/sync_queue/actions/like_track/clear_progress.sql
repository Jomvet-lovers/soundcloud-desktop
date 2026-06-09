-- wanted_state=true: don't clear progress on a row the user flipped to pending-unlike.
UPDATE user_likes_tracks
SET progress  = false,
    synced_at = now()
WHERE user_id = $1
  AND sc_track_id = $2
  AND wanted_state = true
