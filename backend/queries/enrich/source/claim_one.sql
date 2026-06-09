WITH picked AS (SELECT id
                FROM tracks
                WHERE sc_track_id = $3
                  AND (enrich_locked_at IS NULL
                    OR enrich_locked_at < now() - ($1 * interval '1 second'))
                  AND enrich_attempts < $2
                  AND (enrich_state IN ('pending', 'failed')
                    OR (enrich_state = 'done'
                        AND (enriched_at IS NULL
                            OR enriched_at < now() - interval '24 hours')))
    LIMIT 1
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
