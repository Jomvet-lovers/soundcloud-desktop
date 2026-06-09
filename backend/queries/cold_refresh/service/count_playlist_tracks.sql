SELECT COUNT(*) AS "count!"
FROM playlist_tracks
WHERE playlist_urn = $1
