UPDATE artists
SET merged_into = $2
WHERE id = $1
  AND merged_into IS NULL
