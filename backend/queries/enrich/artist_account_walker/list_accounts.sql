SELECT sc_user_id
FROM artist_sc_accounts
WHERE artist_id = $1
  AND role IN ('main', 'alt', 'demo')
