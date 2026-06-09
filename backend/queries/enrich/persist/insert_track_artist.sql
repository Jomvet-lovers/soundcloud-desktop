INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence)
VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (track_id, artist_id, role) DO
UPDATE
    SET position = EXCLUDED.position,
    source = EXCLUDED.source,
    confidence = EXCLUDED.confidence
