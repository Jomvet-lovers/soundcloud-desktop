UPDATE artists
SET sc_user_id = COALESCE(sc_user_id, $2),
    updated_at = now()
WHERE id = $1
