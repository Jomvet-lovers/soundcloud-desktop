INSERT INTO artist_sc_accounts (artist_id, sc_user_id, role, source, verified)
VALUES ($1, $2, $3, $4, $5) ON CONFLICT (artist_id, sc_user_id) DO
UPDATE
    SET role = CASE WHEN EXCLUDED.verified AND NOT artist_sc_accounts.verified
    THEN EXCLUDED.role ELSE artist_sc_accounts.role
END,
       verified = artist_sc_accounts.verified OR EXCLUDED.verified,
       source   = CASE WHEN artist_sc_accounts.source = 'manual'
                       THEN artist_sc_accounts.source ELSE EXCLUDED.source
END,
       updated_at = now()
