-- albums lookup by genius_album_id (enrich::ensure_genius_album). Non-unique:
-- legacy duplicate genius_album_id rows exist, so a UNIQUE build would fail.
-- Pre-create CONCURRENTLY on prod before deploy; IF NOT EXISTS no-ops here.
CREATE INDEX IF NOT EXISTS albums_genius_album_id_idx
    ON albums (genius_album_id)
    WHERE genius_album_id IS NOT NULL;
