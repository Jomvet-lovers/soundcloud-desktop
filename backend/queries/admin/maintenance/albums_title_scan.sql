SELECT id, title, normalized_title
FROM albums
WHERE id > $1
ORDER BY id
LIMIT $2
