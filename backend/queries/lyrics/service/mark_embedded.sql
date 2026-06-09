UPDATE lyrics_cache
SET embedded_at = now()
WHERE sc_track_id = $1
  AND embedded_at IS NULL
