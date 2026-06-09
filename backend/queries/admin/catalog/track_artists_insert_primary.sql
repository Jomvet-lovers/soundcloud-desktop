INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence)
VALUES ($1, $2, 'primary', 0, 'manual', 1.0) ON CONFLICT DO NOTHING
