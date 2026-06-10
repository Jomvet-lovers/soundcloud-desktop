UPDATE tracks
SET metadata_artist = $2
WHERE id = $1
