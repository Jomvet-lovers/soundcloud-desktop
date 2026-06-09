UPDATE tracks
SET transcribe_state = 'done',
    transcribe_at    = now()
WHERE sc_track_id = $1
