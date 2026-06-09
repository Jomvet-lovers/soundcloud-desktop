UPDATE tracks
SET transcribe_state = 'disabled',
    transcribe_at    = now()
WHERE sc_track_id = $1
