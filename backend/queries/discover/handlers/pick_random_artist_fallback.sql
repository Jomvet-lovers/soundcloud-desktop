SELECT id
FROM artists
WHERE merged_into IS NULL
  AND (track_count_primary > 0 OR track_count_featured > 0)
ORDER BY id LIMIT 1
