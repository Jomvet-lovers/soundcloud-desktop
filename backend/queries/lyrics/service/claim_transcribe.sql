UPDATE tracks
SET transcribe_state = 'pending',
    transcribe_at    = now()
WHERE sc_track_id = $1
  AND (transcribe_state IS NULL
    OR (transcribe_state = 'pending' AND transcribe_at < $2))
