SELECT id, title, title_normalized
FROM tracks
WHERE id > $1
ORDER BY id
LIMIT $2
