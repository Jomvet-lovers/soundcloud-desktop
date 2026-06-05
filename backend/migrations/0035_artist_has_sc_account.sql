-- Denormalized "owns a walkable SC account" flag for the account_walk picker,
-- so its claim index spans only walkable artists instead of all of them.
ALTER TABLE artists
    ADD COLUMN IF NOT EXISTS has_sc_account boolean NOT NULL DEFAULT false;

-- Maintained by trigger from any artist_sc_accounts writer; mirrors the picker's
-- role filter ('main','alt','demo').
CREATE
OR REPLACE FUNCTION artist_sc_account_flag() RETURNS trigger AS $$
BEGIN
    IF
TG_OP = 'INSERT' THEN
        IF NEW.role IN ('main', 'alt', 'demo') THEN
UPDATE artists
SET has_sc_account = true
WHERE id = NEW.artist_id
  AND NOT has_sc_account;
END IF;
ELSE
UPDATE artists
SET has_sc_account = EXISTS (SELECT 1
                             FROM artist_sc_accounts a
                             WHERE a.artist_id = OLD.artist_id
                               AND a.role IN ('main', 'alt', 'demo'))
WHERE id = OLD.artist_id;
END IF;
RETURN NULL;
END;
$$
LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS artist_sc_account_flag_trg ON artist_sc_accounts;
CREATE TRIGGER artist_sc_account_flag_trg
    AFTER INSERT OR
UPDATE OF role OR
DELETE
ON artist_sc_accounts
    FOR EACH ROW EXECUTE FUNCTION artist_sc_account_flag();

-- Backfill (pre-run on prod; no-op on fresh DBs and on re-apply).
UPDATE artists a
SET has_sc_account = true
WHERE NOT a.has_sc_account
  AND EXISTS (SELECT 1
              FROM artist_sc_accounts s
              WHERE s.artist_id = a.id
                AND s.role IN ('main', 'alt', 'demo'));

-- Walkable-only claim index, replacing the all-artists artists_account_walk_claim_idx
-- for the picker. Pre-create CONCURRENTLY on prod before deploy.
CREATE INDEX IF NOT EXISTS artists_account_walk_walkable_idx
    ON artists (last_account_walk_at NULLS FIRST)
    WHERE merged_into IS NULL AND has_sc_account;
