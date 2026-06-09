UPDATE wanted_tracks
SET track_id   = t.id,
    status     = 'linked',
    updated_at = now() FROM (SELECT id FROM tracks WHERE sc_track_id = $2 LIMIT 1) t
WHERE wanted_tracks.id = $1
    RETURNING track_id
