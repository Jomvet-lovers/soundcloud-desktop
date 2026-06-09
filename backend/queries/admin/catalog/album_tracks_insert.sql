INSERT INTO album_tracks (album_id, track_id, position)
VALUES ($1, $2, NULL) ON CONFLICT DO NOTHING
