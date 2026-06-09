INSERT INTO artist_sc_accounts (artist_id, sc_user_id, role, source, verified)
VALUES ($1, $2, $3, 'auto_match', false) ON CONFLICT (artist_id, sc_user_id) DO NOTHING
