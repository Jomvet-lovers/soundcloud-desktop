UPDATE artists
SET country    = COALESCE(country, $2),
    avatar_url = COALESCE(avatar_url, $3),
    bio        = COALESCE(bio, $4),
    updated_at = now()
WHERE id = $1
