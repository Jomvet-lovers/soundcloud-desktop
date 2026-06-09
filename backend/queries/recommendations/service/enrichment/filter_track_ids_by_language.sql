SELECT sc_track_id
FROM tracks
WHERE sc_track_id = ANY ($1)
  AND (language IS NULL OR language = ANY ($2))
