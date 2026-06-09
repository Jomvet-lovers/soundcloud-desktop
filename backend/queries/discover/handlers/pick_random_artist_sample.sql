SELECT id
FROM artists TABLESAMPLE BERNOULLI(2)
WHERE merged_into IS NULL
  AND (track_count_primary
    > 0
   OR track_count_featured
    > 0)
    LIMIT 1
