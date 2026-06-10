UPDATE albums
SET primary_artist_id = $2
WHERE primary_artist_id = $1
