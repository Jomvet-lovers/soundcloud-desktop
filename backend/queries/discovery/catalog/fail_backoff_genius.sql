UPDATE artists
SET genius_next_run_at = $3,
    crawl_fail_count   = $2,
    genius_locked_at   = NULL
WHERE id = $1
