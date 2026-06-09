UPDATE sync_queue
SET dead        = true,
    failed_at   = now(),
    locked_at   = NULL,
    last_error  = $1,
    retry_count = $2,
    next_run_at = 'infinity'
WHERE id = $3
