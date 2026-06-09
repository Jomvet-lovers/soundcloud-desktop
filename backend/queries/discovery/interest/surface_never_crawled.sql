UPDATE artists
SET genius_next_run_at = now()
WHERE interest_score > 0
  AND merged_into IS NULL
  AND NOT crawl_dead
  AND genius_artist_id IS NOT NULL
  AND genius_crawled_at IS NULL
  AND genius_next_run_at > now()
