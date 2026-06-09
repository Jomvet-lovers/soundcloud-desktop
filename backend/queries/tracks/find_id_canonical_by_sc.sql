SELECT id, canonical_track_id
FROM tracks
WHERE sc_track_id = $1
