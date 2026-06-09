INSERT INTO wanted_track_albums (wanted_track_id, album_id, position)
VALUES ($1, $2, $3) ON CONFLICT DO NOTHING
