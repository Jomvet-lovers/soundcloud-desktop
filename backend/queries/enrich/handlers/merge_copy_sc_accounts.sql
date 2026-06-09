INSERT INTO artist_sc_accounts (artist_id, sc_user_id, role, source, verified, notes)
SELECT $1, sc_user_id, role, source, verified, notes
FROM artist_sc_accounts
WHERE artist_id = $2 ON CONFLICT DO NOTHING
