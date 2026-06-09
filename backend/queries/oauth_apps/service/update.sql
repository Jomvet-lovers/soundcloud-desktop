-- COALESCE-guarded partial update; null params keep existing column values.
UPDATE oauth_apps
SET name          = COALESCE($2, name),
    client_id     = COALESCE($3, client_id),
    client_secret = COALESCE($4, client_secret),
    redirect_uri  = COALESCE($5, redirect_uri),
    active        = COALESCE($6, active),
    updated_at    = now()
WHERE id = $1 RETURNING id, name, client_id, client_secret, redirect_uri, active, last_used_at, created_at, updated_at
