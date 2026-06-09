UPDATE tracks
SET canonical_track_id = $1
WHERE isrc = $2
  AND canonical_track_id IS NULL
