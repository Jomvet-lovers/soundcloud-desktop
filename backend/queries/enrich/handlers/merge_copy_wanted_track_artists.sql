INSERT INTO wanted_track_artists (wanted_track_id, artist_id, role, position)
SELECT wanted_track_id, $1, role, position
FROM wanted_track_artists
WHERE artist_id = $2 ON CONFLICT DO NOTHING
