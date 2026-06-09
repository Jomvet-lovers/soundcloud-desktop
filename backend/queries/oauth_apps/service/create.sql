INSERT INTO oauth_apps (id, name, client_id, client_secret, redirect_uri, active)
VALUES ($1, $2, $3, $4, $5,
        $6) RETURNING id, name, client_id, client_secret, redirect_uri, active, last_used_at, created_at, updated_at
