UPDATE tracks
SET title_normalized = $2
WHERE id = $1
