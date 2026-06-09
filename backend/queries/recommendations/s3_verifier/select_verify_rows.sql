SELECT sc_track_id, s3_verified_at, s3_missing_at
FROM tracks
WHERE sc_track_id = ANY ($1)
