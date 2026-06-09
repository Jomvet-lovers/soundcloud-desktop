SELECT track_data, created_at
FROM disliked_tracks
WHERE sc_user_id = ANY ($1)
ORDER BY created_at DESC LIMIT $2
