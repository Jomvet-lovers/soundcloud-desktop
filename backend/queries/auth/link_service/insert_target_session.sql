INSERT INTO sessions
(id, access_token, refresh_token, expires_at, scope,
 soundcloud_user_id, username, oauth_app_id)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id, access_token, refresh_token, expires_at, scope,
          soundcloud_user_id, username, oauth_app_id, created_at, updated_at
