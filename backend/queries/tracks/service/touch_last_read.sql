UPDATE tracks
SET last_read_at = now()
WHERE sc_track_id = $1
  AND (last_read_at IS NULL
    OR last_read_at < now() - INTERVAL '5 minutes')
