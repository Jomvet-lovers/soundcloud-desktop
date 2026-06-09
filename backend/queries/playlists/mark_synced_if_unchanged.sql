UPDATE playlists
SET synced_rev = $2
WHERE urn = $1
  AND desired_rev = $2
