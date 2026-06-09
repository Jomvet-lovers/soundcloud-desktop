SELECT access_token AS "access_token!"
FROM oauth_app_tokens
WHERE expires_at > $1
  AND access_token IS NOT NULL
