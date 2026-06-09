UPDATE tracks
SET storage_attempts   = storage_attempts +
                         CASE WHEN storage_state IN ('pending', 'missing') THEN 1 ELSE 0 END,
    storage_state      = CASE
                             WHEN storage_state IN ('pending', 'missing') AND storage_attempts + 1 >= $2
                                 THEN 'failed'
                             ELSE storage_state
                         END,
    hq_upgrade_pending = false,
    updated_at         = now()
WHERE sc_track_id = $1
