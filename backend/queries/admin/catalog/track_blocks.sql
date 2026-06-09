-- BlockRow field order
SELECT b.artist_id, a.name AS "name?", b.note, b.created_at
FROM track_artist_blocks b
         LEFT JOIN artists a ON a.id = b.artist_id
WHERE b.track_id = $1
ORDER BY b.created_at DESC
