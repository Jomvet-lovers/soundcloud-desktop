SELECT normalized_name
FROM artists
WHERE normalized_name = ANY ($1)
  AND merged_into IS NULL
