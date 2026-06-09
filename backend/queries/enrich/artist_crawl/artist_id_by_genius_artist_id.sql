SELECT id
FROM artists
WHERE genius_artist_id = $1 LIMIT 1
