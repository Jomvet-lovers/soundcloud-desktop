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
ORDER BY created_at ASC
