SELECT id
FROM albums TABLESAMPLE BERNOULLI(2)
WHERE track_count
    > 0
  AND popularity_score
    > 0
  AND primary_artist_id IS NOT NULL
    LIMIT 1
