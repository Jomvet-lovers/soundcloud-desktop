UPDATE tracks
SET needs_duration_resolve = false,
    updated_at             = now()
WHERE sc_track_id = $1
