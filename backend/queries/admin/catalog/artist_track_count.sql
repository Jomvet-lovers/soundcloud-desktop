SELECT COUNT(*) ::int8 AS "count!"
FROM track_artists
WHERE artist_id = $1
