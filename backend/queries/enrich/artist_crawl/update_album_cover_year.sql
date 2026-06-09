UPDATE albums
SET cover_url    = COALESCE(cover_url, $2),
    release_year = COALESCE(release_year, $3),
    updated_at   = now()
WHERE id = $1
