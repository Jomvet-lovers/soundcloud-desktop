UPDATE sessions
SET access_token  = $2,
    refresh_token = $3,
    expires_at    = $4,
    scope         = $5,
    updated_at    = now()
WHERE id = $1 RETURNING id,
          access_token,
          refresh_token,
          expires_at,
          scope,
          soundcloud_user_id,
          username,
          oauth_app_id,
          created_at,
          updated_at
