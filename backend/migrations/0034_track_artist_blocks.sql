-- Sticky "detach": an admin can permanently unlink a (track, artist) pair so the
-- enrich/crawl pipeline never re-links it, even on an exact 1:1 parse. Enforced by
-- triggers so it holds across every insert path (persist, account walker, merge).

CREATE TABLE "track_artist_blocks"
(
    "track_id"   uuid        NOT NULL REFERENCES "tracks" ("id") ON DELETE CASCADE,
    "artist_id"  uuid        NOT NULL REFERENCES "artists" ("id") ON DELETE CASCADE,
    "note"       text,
    "created_at" timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY ("track_id", "artist_id")
);
CREATE INDEX "track_artist_blocks_artist_idx" ON "track_artist_blocks" ("artist_id");

-- Skip inserting a blocked credit (BEFORE INSERT → RETURN NULL drops just that row).
CREATE
OR REPLACE FUNCTION track_artists_block_guard() RETURNS trigger AS $$
BEGIN
    IF
EXISTS (
        SELECT 1 FROM track_artist_blocks b
        WHERE b.track_id = NEW.track_id AND b.artist_id = NEW.artist_id
    ) THEN
        RETURN NULL;
END IF;
RETURN NEW;
END;
$$
LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS track_artists_block_guard_trg ON track_artists;
CREATE TRIGGER track_artists_block_guard_trg
    BEFORE INSERT
    ON track_artists
    FOR EACH ROW EXECUTE FUNCTION track_artists_block_guard();

-- Keep the denormalized tracks.primary_artist_id from pointing at a blocked artist.
CREATE
OR REPLACE FUNCTION tracks_primary_block_guard() RETURNS trigger AS $$
BEGIN
    IF
NEW.primary_artist_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM track_artist_blocks b
        WHERE b.track_id = NEW.id AND b.artist_id = NEW.primary_artist_id
    ) THEN
        NEW.primary_artist_id := NULL;
END IF;
RETURN NEW;
END;
$$
LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS tracks_primary_block_guard_trg ON tracks;
CREATE TRIGGER tracks_primary_block_guard_trg
    BEFORE INSERT OR
UPDATE OF primary_artist_id
ON tracks
    FOR EACH ROW EXECUTE FUNCTION tracks_primary_block_guard();
