SELECT id, metadata_artist
FROM tracks
WHERE metadata_artist IS NOT NULL
  AND position('\u' in metadata_artist) > 0
  AND id > $1
ORDER BY id
LIMIT $2
