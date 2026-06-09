SELECT COUNT(*) ::int8 AS "count!"
FROM album_artists
WHERE artist_id = $1
