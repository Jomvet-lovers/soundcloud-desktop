INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence)
VALUES ($1, $2, $3, COALESCE($4, 0), 'manual', 1.0) ON CONFLICT (track_id, artist_id, role)
DO
UPDATE SET position = EXCLUDED.position, source = 'manual', confidence = 1.0
