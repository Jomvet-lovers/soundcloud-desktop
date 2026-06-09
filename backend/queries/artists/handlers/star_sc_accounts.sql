SELECT sc_user_id, role, source, verified
FROM artist_sc_accounts
WHERE artist_id = $1
  AND role IN ('main', 'demo')
