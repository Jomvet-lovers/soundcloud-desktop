SELECT expires_at, refresh_attempts
FROM oauth_app_tokens
WHERE oauth_app_id = $1
