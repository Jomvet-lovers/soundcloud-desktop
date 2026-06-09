SELECT sc_track_id
FROM disliked_tracks
WHERE sc_user_id = ANY ($1) LIMIT $2
