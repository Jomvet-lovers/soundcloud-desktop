UPDATE artists
SET mb_crawled_at    = now(),
    mb_next_run_at   = now() + ($2 * interval '1 day'),
    mb_locked_at     = NULL,
    crawl_fail_count = 0
WHERE id = $1
