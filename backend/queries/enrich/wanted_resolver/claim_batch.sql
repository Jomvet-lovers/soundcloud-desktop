WITH picked AS (SELECT id
                FROM wanted_tracks
                WHERE status = 'wanted'
                  AND track_id IS NULL
                  AND resolve_next_run_at <= now()
                  AND (resolve_locked_at IS NULL
                    OR resolve_locked_at < now() - interval '10 minutes')
                ORDER BY resolve_next_run_at
    LIMIT $1 FOR
UPDATE SKIP LOCKED
    )
UPDATE wanted_tracks w
SET resolve_locked_at = now(),
    resolve_attempts  = w.resolve_attempts + 1 FROM picked
WHERE w.id = picked.id
    RETURNING w.id
