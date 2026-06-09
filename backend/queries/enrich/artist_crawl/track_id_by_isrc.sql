SELECT id
FROM tracks
WHERE isrc = $1 LIMIT 1
