UPDATE tracks
SET album_id       = COALESCE(album_id, $2),
    album_position = COALESCE(album_position, $3)
WHERE id = $1
