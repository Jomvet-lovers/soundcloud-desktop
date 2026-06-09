UPDATE artists
SET crawl_dead         = false,
    crawl_fail_count   = 0,
    genius_next_run_at = now(),
    mb_next_run_at     = now(),
    genius_locked_at   = NULL,
    mb_locked_at       = NULL,
    updated_at         = now()
WHERE id = $1
  AND merged_into IS NULL
