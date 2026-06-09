UPDATE tracks
SET enrich_state     = 'dead',
    enrich_locked_at = NULL,
    enrich_error     = $2,
    enriched_at      = now()
WHERE id = $1
