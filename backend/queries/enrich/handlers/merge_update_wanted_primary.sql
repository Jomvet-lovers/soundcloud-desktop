UPDATE wanted_tracks
SET primary_artist_id = $1
WHERE primary_artist_id = $2
