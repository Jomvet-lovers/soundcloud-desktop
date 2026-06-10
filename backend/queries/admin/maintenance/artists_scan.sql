SELECT id, name, normalized_name
FROM artists
WHERE merged_into IS NULL
  AND id > $1
ORDER BY id
LIMIT $2
