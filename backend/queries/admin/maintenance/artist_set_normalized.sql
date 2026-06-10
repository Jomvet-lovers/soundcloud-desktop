UPDATE artists
SET normalized_name = $2
WHERE id = $1
