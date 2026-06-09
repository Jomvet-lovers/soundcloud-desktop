UPDATE sync_queue
SET locked_at   = NULL,
    last_error  = $1,
    next_run_at = now() + ($2::bigint || ' seconds')::interval
WHERE id = $3
