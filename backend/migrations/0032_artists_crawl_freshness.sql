-- Per-source crawl freshness scheduler on artists. Two independent lanes
-- (MB serialized 1.1s, Genius proxy-parallel) each get their own crawled_at /
-- next_run_at / locked_at so one never blocks the other. Eligibility drops the
-- old confidence floor + lifetime crawl_attempts odometer (the documented cause
-- of "79% of artists never crawled"): an artist is reachable as long as it has
-- the external id, is not merged, and is not crawl_dead. crawl_fail_count counts
-- CONSECUTIVE failures (reset on success), distinct from a lifetime cap.
-- interest_score (precomputed) scales the re-crawl interval so popular artists
-- refresh more often without a per-tick user_events join.
ALTER TABLE artists
    ADD COLUMN IF NOT EXISTS mb_crawled_at timestamptz,
    ADD COLUMN IF NOT EXISTS genius_crawled_at timestamptz,
    ADD COLUMN IF NOT EXISTS mb_next_run_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS genius_next_run_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS mb_locked_at timestamptz,
    ADD COLUMN IF NOT EXISTS genius_locked_at timestamptz,
    ADD COLUMN IF NOT EXISTS crawl_fail_count smallint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS crawl_dead boolean NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS interest_score real NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS account_walk_locked_at timestamptz;

-- Seed per-source crawled_at from the legacy combined cursor so coverage
-- metrics are continuous (these are ~72k/4k row writes, run at startup).
UPDATE artists
SET genius_crawled_at = last_crawled_at
WHERE last_crawled_at IS NOT NULL
  AND genius_artist_id IS NOT NULL
  AND genius_crawled_at IS NULL;
UPDATE artists
SET mb_crawled_at = last_crawled_at
WHERE last_crawled_at IS NOT NULL
  AND mb_artist_id IS NOT NULL
  AND mb_crawled_at IS NULL;

-- De-thunder the first full sweep: every artist with an external id becomes due
-- within 24h (jittered) instead of all-at-once on the first tick.
UPDATE artists
SET genius_next_run_at = now() + (random() * interval '24 hours')
WHERE merged_into IS NULL
  AND genius_artist_id IS NOT NULL;
UPDATE artists
SET mb_next_run_at = now() + (random() * interval '24 hours')
WHERE merged_into IS NULL
  AND mb_artist_id IS NOT NULL;

-- Lane claim indexes: order by <source>_next_run_at, filter the eligible set.
-- locked_at deliberately not in the predicate so lease-expiry reclaim uses the
-- same index; in-flight rows are bounded by lane concurrency.
-- On prod create CONCURRENTLY before deploy; IF NOT EXISTS no-ops at startup.
CREATE INDEX IF NOT EXISTS artists_genius_claim_idx
    ON artists (genius_next_run_at)
    WHERE merged_into IS NULL AND NOT crawl_dead AND genius_artist_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS artists_mb_claim_idx
    ON artists (mb_next_run_at)
    WHERE merged_into IS NULL AND NOT crawl_dead AND mb_artist_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS artists_account_walk_claim_idx
    ON artists (last_account_walk_at NULLS FIRST)
    WHERE merged_into IS NULL;
