SELECT (desired_rev > synced_rev) AS "pending!"
FROM playlists
WHERE urn = $1
