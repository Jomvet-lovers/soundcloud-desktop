UPDATE tracks
SET sharing    = $2,
    updated_at = now()
WHERE sc_track_id = $1
