UPDATE oauth_apps
SET last_used_at = $1,
    updated_at   = now()
WHERE id = $2 RETURNING id, name, client_id, client_secret, redirect_uri, active, last_used_at, created_at, updated_at
