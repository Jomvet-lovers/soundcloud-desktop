SELECT transcribe_state, transcribe_at
FROM tracks
WHERE sc_track_id = $1
