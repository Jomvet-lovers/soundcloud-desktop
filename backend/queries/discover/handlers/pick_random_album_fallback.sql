SELECT id
FROM albums
WHERE track_count > 0
  AND popularity_score > 0
  AND primary_artist_id IS NOT NULL
ORDER BY id LIMIT 1
