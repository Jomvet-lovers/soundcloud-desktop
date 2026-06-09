SELECT COUNT(*) ::int8 AS "count!"
FROM artist_sc_accounts
WHERE artist_id = $1
  AND role = 'main'
