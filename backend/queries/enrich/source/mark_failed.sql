UPDATE tracks
SET enrich_state       = 'failed',
    enrich_locked_at   = NULL,
    enrich_next_run_at = $2,
    enrich_error       = $3
WHERE id = $1
