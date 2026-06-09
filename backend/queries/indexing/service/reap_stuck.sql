SELECT sc_track_id
FROM tracks
WHERE created_at < $1
  AND (
    storage_state = 'pending'
        OR (index_state = 'pending'
        AND storage_state = 'ok'
        AND s3_verified_at IS NOT NULL)
    )
ORDER BY index_priority, created_at LIMIT $2
