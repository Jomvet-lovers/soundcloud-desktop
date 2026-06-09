SELECT id, name
FROM artists
WHERE normalized_name = $1
  AND merged_into IS NULL LIMIT 1
