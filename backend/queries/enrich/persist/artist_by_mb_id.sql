SELECT id
FROM artists
WHERE mb_artist_id = $1 LIMIT 1
