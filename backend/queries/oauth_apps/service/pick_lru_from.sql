SELECT id,
       name,
       client_id,
       client_secret,
       redirect_uri,
       active,
       last_used_at,
       created_at,
       updated_at
FROM oauth_apps
WHERE active = true
  AND id = ANY ($1)
ORDER BY last_used_at ASC NULLS FIRST, created_at ASC LIMIT 1 FOR
UPDATE SKIP LOCKED
