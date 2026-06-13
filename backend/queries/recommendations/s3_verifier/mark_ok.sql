UPDATE tracks
SET storage_state  = 'ok',
    s3_verified_at = now(),
    s3_missing_at  = NULL
WHERE sc_track_id = ANY ($1)
  AND storage_state <> 'too_long'
