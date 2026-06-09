UPDATE wanted_tracks
SET status     = $1,
    updated_at = now()
WHERE id = $2
