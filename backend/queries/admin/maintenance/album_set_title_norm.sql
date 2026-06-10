UPDATE albums
SET normalized_title = $2
WHERE id = $1
