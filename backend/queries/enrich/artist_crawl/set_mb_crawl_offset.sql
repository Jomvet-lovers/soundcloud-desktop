UPDATE artists
SET mb_crawl_offset = $2
WHERE id = $1
