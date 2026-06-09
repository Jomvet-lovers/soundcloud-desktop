SELECT id, name, 'primary'::text AS "role!", avatar_url
FROM artists
WHERE id = $1
  AND merged_into IS NULL
