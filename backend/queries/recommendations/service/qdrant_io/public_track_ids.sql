SELECT sc_track_id
FROM tracks
WHERE sc_track_id = ANY ($1)
  AND sharing = 'public'
