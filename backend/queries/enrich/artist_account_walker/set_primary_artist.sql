UPDATE tracks
SET primary_artist_id = $2,
    updated_at        = now()
WHERE id = $1
  AND primary_artist_id IS NULL
