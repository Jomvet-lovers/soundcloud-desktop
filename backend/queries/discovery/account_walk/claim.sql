WITH picked AS (SELECT ar.id
                FROM artists ar
                WHERE ar.merged_into IS NULL
                  AND (ar.last_account_walk_at IS NULL
                    OR ar.last_account_walk_at < now() - ($1::bigint * interval '1 day'))
                  AND (ar.account_walk_locked_at IS NULL
                    OR ar.account_walk_locked_at < now() - ($2::bigint * interval '1 second'))
                  AND ar.has_sc_account
                ORDER BY ar.last_account_walk_at NULLS FIRST
    LIMIT $3
    FOR
UPDATE SKIP LOCKED
    )
UPDATE artists ar
SET account_walk_locked_at = now() FROM picked
WHERE ar.id = picked.id
    RETURNING ar.id
    , ar.name
