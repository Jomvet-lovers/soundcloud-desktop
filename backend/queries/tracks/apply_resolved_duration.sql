UPDATE tracks
SET duration_ms            = $2,
    needs_duration_resolve = false,
    storage_state          = CASE
                                 WHEN storage_state = 'failed' AND duration_ms <> $2 THEN 'pending'
                                 ELSE storage_state
                             END,
    storage_attempts       = CASE
                                 WHEN storage_state = 'failed' AND duration_ms <> $2 THEN 0
                                 ELSE storage_attempts
                             END,
    updated_at             = now()
WHERE sc_track_id = $1
