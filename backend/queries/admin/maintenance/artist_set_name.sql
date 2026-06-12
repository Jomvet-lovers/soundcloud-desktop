UPDATE artists
SET name = $2, normalized_name = $3
WHERE id = $1
