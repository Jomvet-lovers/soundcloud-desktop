UPDATE tracks
SET storage_state = CASE
                        WHEN storage_state = 'pending' THEN 'pending'
                        ELSE 'missing'
    END,
    s3_missing_at = now()
WHERE sc_track_id = ANY ($1)
  AND storage_state <> 'too_long'
