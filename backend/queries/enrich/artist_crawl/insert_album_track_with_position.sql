INSERT INTO album_tracks (album_id, track_id, position)
VALUES ($1, $2, $3) ON CONFLICT DO NOTHING
