UPDATE sync_queue
SET next_run_at = now(),
    locked_at   = NULL
WHERE locked_at IS NULL
   OR locked_at < now() - interval '5 minutes'
