UPDATE artists
SET genius_crawl_offset = $2
WHERE id = $1
