-- Enrich claim substrate: split the overloaded enriched_at into a dedicated
-- lease (enrich_locked_at) + a sargable backoff cursor (enrich_next_run_at),
-- and a real error column so failures stop corrupting enrich_source.
-- enriched_at becomes purely the success timestamp. enrich_state gains 'dead'
-- (terminal at the attempt cap); varchar(16) already fits.
ALTER TABLE tracks
    ADD COLUMN IF NOT EXISTS enrich_locked_at timestamptz,
    ADD COLUMN IF NOT EXISTS enrich_next_run_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS enrich_error text;

-- Claim index matching ORDER BY index_priority, enrich_next_run_at. Partial on
-- the non-terminal states only (done/dead excluded), so the working set shrinks
-- monotonically. locked_at is NOT in the predicate so lease-expiry reclaim
-- (locked_at < cutoff) uses the same index; in-flight rows are bounded by the
-- worker concurrency and stay negligibly small.
-- On prod create CONCURRENTLY before deploy; this is then a no-op at startup.
CREATE INDEX IF NOT EXISTS tracks_enrich_claim_idx
    ON tracks (index_priority, enrich_next_run_at)
    WHERE enrich_state IN ('pending', 'failed');
