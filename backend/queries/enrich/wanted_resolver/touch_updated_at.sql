UPDATE wanted_tracks
SET updated_at = now()
WHERE id = $1
