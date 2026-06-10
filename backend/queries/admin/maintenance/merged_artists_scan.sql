SELECT id, merged_into AS "merged_into!"
FROM artists
WHERE merged_into IS NOT NULL AND id > $1
ORDER BY id
LIMIT $2
