SELECT role
FROM artist_sc_accounts
WHERE artist_id = $1
  AND sc_user_id = $2
