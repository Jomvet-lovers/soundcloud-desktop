UPDATE artists
SET merged_into = $1,
    updated_at  = now()
WHERE id = $2
  AND merged_into IS NULL
