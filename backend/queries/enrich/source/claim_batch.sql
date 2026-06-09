WITH picked AS (SELECT id
                FROM tracks
                WHERE enrich_state IN ('pending', 'failed')
                  AND enrich_next_run_at <= now()
                  AND (enrich_locked_at IS NULL
                    OR enrich_locked_at < now() - ($1 * interval '1 second'))
                  AND enrich_attempts < $2
                ORDER BY index_priority, enrich_next_run_at
    LIMIT $3
    FOR
UPDATE SKIP LOCKED
    )
UPDATE tracks t
SET enrich_locked_at = now(),
    enrich_attempts  = t.enrich_attempts + 1 FROM picked
WHERE t.id = picked.id
    RETURNING t.id
    , t.sc_track_id
    , t.enrich_attempts
