UPDATE tracks
SET enrich_state       = 'pending',
    enrich_attempts    = 0,
    enriched_at        = NULL,
    enrich_source      = NULL,
    enrich_confidence  = NULL,
    enrich_next_run_at = now(),
    enrich_locked_at   = NULL,
    enrich_error       = NULL
WHERE sc_track_id = $1
