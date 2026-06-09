UPDATE artists
SET crawl_dead       = true,
    crawl_fail_count = $2,
    genius_locked_at = NULL
WHERE id = $1
