SELECT canonical_track_id AS "canonical_track_id!"
FROM tracks
WHERE isrc = $1
  AND canonical_track_id IS NOT NULL
  AND id <> $2 LIMIT 1
