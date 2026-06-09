INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence)
SELECT track_id, $1, role, position, source, confidence
FROM track_artists
WHERE artist_id = $2 ON CONFLICT DO NOTHING
