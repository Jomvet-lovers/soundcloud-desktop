SELECT COUNT(*) ::int8 AS "count!"
FROM track_artists
WHERE track_id = $1
