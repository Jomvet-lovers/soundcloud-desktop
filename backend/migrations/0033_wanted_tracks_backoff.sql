-- wanted_tracks had no attempts/next_run_at/lease, so dead wants were
-- re-searched every cycle forever. Add the claim substrate so WantedResolveSource
-- backs off and retires dead wants to status='unresolvable' at the attempt cap.
ALTER TABLE wanted_tracks
    ADD COLUMN IF NOT EXISTS resolve_attempts smallint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS resolve_next_run_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS resolve_locked_at timestamptz,
    ADD COLUMN IF NOT EXISTS resolve_error text;

-- On prod create CONCURRENTLY before deploy; IF NOT EXISTS no-ops at startup.
CREATE INDEX IF NOT EXISTS wanted_resolve_claim_idx
    ON wanted_tracks (resolve_next_run_at)
    WHERE status = 'wanted' AND track_id IS NULL;
