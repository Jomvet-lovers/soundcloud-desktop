SELECT primary_artist_id AS "primary_artist_id!"
FROM tracks
WHERE sc_track_id = $1
  AND primary_artist_id IS NOT NULL LIMIT 1
