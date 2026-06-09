SELECT id
FROM albums
WHERE mb_release_id = $1 LIMIT 1
