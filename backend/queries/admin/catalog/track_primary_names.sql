SELECT ta.track_id AS "track_id!",
       a.name      AS "name!"
FROM track_artists ta
         JOIN artists a ON a.id = ta.artist_id
WHERE ta.track_id = ANY ($1)
  AND ta.role = 'primary'
ORDER BY ta.track_id, ta.position
