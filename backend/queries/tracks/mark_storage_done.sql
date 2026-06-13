UPDATE tracks
SET storage_state      = 'ok',
    storage_quality    = COALESCE($2, storage_quality),
    s3_verified_at     = now(),
    s3_missing_at      = NULL,
    storage_attempts   = 0,
    hq_upgrade_pending = CASE
                             WHEN $2 = 'hq' THEN false
                             WHEN $2 = 'sq' AND tracks.storage_quality IS DISTINCT
FROM 'hq' THEN true
    ELSE hq_upgrade_pending
END
,
    updated_at = now()
WHERE sc_track_id = $1
  AND storage_state <> 'too_long'
