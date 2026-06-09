UPDATE artist_sc_accounts
SET notes      = $3,
    updated_at = now()
WHERE artist_id = $1
  AND sc_user_id = $2
