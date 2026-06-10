SELECT urn, title, title_normalized
FROM playlists
WHERE urn > $1
ORDER BY urn
LIMIT $2
