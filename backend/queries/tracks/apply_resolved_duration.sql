UPDATE tracks
SET duration_ms            = $2,
    needs_duration_resolve = false,
    updated_at             = now()
WHERE sc_track_id = $1
