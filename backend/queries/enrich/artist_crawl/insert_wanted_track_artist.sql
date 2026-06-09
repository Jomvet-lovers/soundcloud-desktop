INSERT INTO wanted_track_artists (wanted_track_id, artist_id, role, position)
VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING
