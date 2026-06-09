UPDATE artists
SET last_crawled_at = now(),
    crawl_attempts  = crawl_attempts + 1
WHERE id = $1
  AND merged_into IS NULL
  AND (last_crawled_at IS NULL OR last_crawled_at < now() - interval '5 minutes') RETURNING id AS "id!",
          mb_artist_id,
          genius_artist_id,
          sc_user_id,
          mb_crawl_offset AS "mb_crawl_offset!",
          genius_crawl_offset AS "genius_crawl_offset!"
