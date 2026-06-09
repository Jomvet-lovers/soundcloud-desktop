SELECT COUNT(*)::int8 AS "artists_total!", COUNT(*) FILTER (WHERE genius_artist_id IS NOT NULL)::int8 AS "genius_total!", COUNT(*) FILTER (WHERE genius_crawled_at IS NOT NULL)::int8 AS "genius_crawled!", COUNT(*) FILTER (WHERE mb_artist_id IS NOT NULL)::int8 AS "mb_total!", COUNT(*) FILTER (WHERE mb_crawled_at IS NOT NULL)::int8 AS "mb_crawled!", COUNT(*) FILTER (WHERE NOT crawl_dead AND (genius_artist_id IS NOT NULL OR mb_artist_id IS NOT NULL)
                       AND (genius_next_run_at <= now() OR mb_next_run_at <= now()))::int8 AS "due_now!", COUNT(*) FILTER (WHERE crawl_dead)::int8 AS "dead!"
FROM artists
WHERE merged_into IS NULL
