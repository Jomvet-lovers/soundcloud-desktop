DELETE
FROM track_artist_blocks
WHERE track_id = $1
  AND artist_id = $2
