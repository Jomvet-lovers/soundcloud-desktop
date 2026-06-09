INSERT INTO album_tracks (album_id, track_id)
VALUES ($1, $2) ON CONFLICT DO NOTHING
