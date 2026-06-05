-- Superseded indexes: the enrich pickup pair predates the work-scheduler claim
-- index (tracks_enrich_claim_idx, 0031) and is no longer chosen; the counters
-- fetched_at index has no predicate/ordering reader. Drop to cut write
-- amplification on tracks and sc_track_counters. Pre-dropped CONCURRENTLY on
-- prod; DROP IF EXISTS no-ops here.
DROP INDEX IF EXISTS tracks_enrich_pickup_idx;
DROP INDEX IF EXISTS tracks_enrich_pickup_pri_idx;
DROP INDEX IF EXISTS sc_track_counters_fetched_at_idx;
