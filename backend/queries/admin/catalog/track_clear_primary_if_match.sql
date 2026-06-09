UPDATE tracks
SET primary_artist_id = NULL
WHERE id = $1
  AND primary_artist_id = $2
