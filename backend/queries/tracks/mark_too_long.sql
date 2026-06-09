UPDATE tracks
SET storage_state    = 'too_long',
    index_state      = 'too_long',
    transcribe_state = 'disabled',
    updated_at       = now()
WHERE sc_track_id = $1
  AND (storage_state <> 'too_long' OR index_state <> 'too_long')
