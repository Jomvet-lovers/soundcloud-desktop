SELECT id,
       access_token,
       refresh_token,
       expires_at,
       scope,
       soundcloud_user_id,
       username,
       oauth_app_id,
       created_at,
       updated_at
FROM sessions
WHERE id = $1
