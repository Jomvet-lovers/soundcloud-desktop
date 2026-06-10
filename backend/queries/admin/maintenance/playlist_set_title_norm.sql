UPDATE playlists
SET title_normalized = $2
WHERE urn = $1
