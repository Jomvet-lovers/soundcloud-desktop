-- Захват батча: per-(user,target) сериализация через NOT EXISTS на живой lease.
UPDATE sync_queue
SET locked_at = now()
WHERE id IN (SELECT q.id
             FROM sync_queue q
             WHERE q.dead = false
               AND (q.locked_at IS NULL OR q.locked_at < $1)
               AND q.next_run_at <= now()
               AND NOT EXISTS (SELECT 1
                               FROM sync_queue s
                               WHERE s.user_id = q.user_id
                                 AND s.target_urn = q.target_urn
                                 AND s.id <> q.id
                                 AND s.locked_at IS NOT NULL
                                 AND s.locked_at >= $1)
             ORDER BY q.next_run_at ASC, q.created_at ASC
    FOR
UPDATE SKIP LOCKED
    LIMIT $2
    )
    RETURNING
    id,
    user_id,
    action_type,
    target_urn,
    payload,
    locked_at,
    retry_count,
    last_error,
    next_run_at,
    created_at,
    dead,
    failed_at
