UPDATE artists
SET avatar_url = COALESCE(avatar_url, $2),
    updated_at = now()
WHERE id = $1
