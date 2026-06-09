INSERT INTO oauth_app_tokens
(oauth_app_id, access_token, expires_at,
 refreshed_at, refresh_attempts, last_refresh_error)
VALUES ($1, NULL, now(), now(), 1, $2) ON CONFLICT (oauth_app_id) DO
UPDATE SET
    refresh_attempts = oauth_app_tokens.refresh_attempts + 1,
    last_refresh_error = EXCLUDED.last_refresh_error,
    refreshed_at = now()
