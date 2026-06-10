SELECT id
FROM artists
WHERE normalized_name = $1
  AND merged_into IS NULL
  AND id <> $2
