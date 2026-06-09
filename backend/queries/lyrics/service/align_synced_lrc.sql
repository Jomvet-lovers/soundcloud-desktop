UPDATE lyrics_cache
SET synced_lrc = $2
WHERE sc_track_id = $1
  AND synced_lrc IS NULL
