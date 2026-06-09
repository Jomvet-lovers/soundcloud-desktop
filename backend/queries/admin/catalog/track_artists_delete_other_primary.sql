DELETE
FROM track_artists
WHERE track_id = $1
  AND role = 'primary'
  AND artist_id <> $2
