INSERT INTO artist_sc_accounts (artist_id, sc_user_id, role, source, verified)
VALUES ($1, $2, 'alt', 'reupload_pattern', false) ON CONFLICT (artist_id, sc_user_id) DO NOTHING
