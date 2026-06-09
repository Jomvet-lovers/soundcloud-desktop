UPDATE artists
SET mb_next_run_at   = $3,
    crawl_fail_count = $2,
    mb_locked_at     = NULL
WHERE id = $1
