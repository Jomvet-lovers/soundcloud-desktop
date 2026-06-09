SELECT status, COUNT(*) ::int8 AS "count!"
FROM wanted_tracks
GROUP BY status
ORDER BY COUNT(*) DESC
