-- TrackCreditRow field order
SELECT ta.artist_id, a.name AS "name?", ta.role, ta.position, ta.source
FROM track_artists ta
         LEFT JOIN artists a ON a.id = ta.artist_id
WHERE ta.track_id = $1
ORDER BY ta.role, ta.position
