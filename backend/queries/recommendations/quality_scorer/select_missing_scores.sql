SELECT sc_track_id
FROM tracks
WHERE indexed_at IS NOT NULL
  AND quality_score IS NULL
ORDER BY indexed_at DESC LIMIT $1
