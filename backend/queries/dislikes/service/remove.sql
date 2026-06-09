DELETE
FROM disliked_tracks
WHERE sc_user_id = ANY ($1)
  AND sc_track_id = $2
