SELECT id
FROM albums
WHERE genius_album_id = $1 LIMIT 1
