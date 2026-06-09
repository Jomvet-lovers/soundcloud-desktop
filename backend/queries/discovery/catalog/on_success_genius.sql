UPDATE artists
SET genius_crawled_at  = now(),
    genius_next_run_at = now() + ($2 * interval '1 day'),
    genius_locked_at   = NULL,
    mb_crawled_at      = CASE WHEN mb_artist_id IS NOT NULL THEN now() ELSE mb_crawled_at END,
    mb_next_run_at     = CASE
                             WHEN mb_artist_id IS NOT NULL
                                 THEN now() + ($2 * interval '1 day')
                             ELSE mb_next_run_at END,
    crawl_fail_count   = 0
WHERE id = $1
