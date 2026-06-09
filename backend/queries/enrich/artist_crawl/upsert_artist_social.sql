INSERT INTO artist_socials (artist_id, kind, url, source)
VALUES ($1, $2, $3, $4) ON CONFLICT (artist_id, url) DO
UPDATE
    SET kind = EXCLUDED.kind,
    source = EXCLUDED.source,
    updated_at = now()
