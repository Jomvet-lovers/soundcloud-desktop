UPDATE wanted_tracks
SET resolve_locked_at = NULL
WHERE id = ANY ($1)
