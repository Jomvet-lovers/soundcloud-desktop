UPDATE tracks
SET album_id = COALESCE(album_id, $2)
WHERE id = $1
