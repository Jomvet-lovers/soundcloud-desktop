UPDATE tracks
SET language            = $2,
    language_confidence = $3
WHERE sc_track_id = $1
