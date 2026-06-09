-- ScAccountRow field order
SELECT sc_user_id, role, source, verified, notes
FROM artist_sc_accounts
WHERE artist_id = $1
ORDER BY role, sc_user_id
