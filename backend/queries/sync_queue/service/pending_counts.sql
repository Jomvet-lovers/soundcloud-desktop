SELECT COUNT(*) FILTER (WHERE retry_count = 0 AND dead = false)::bigint AS "pending!", COUNT(*) FILTER (WHERE retry_count > 0 OR dead = true)::bigint AS "failed!"
FROM sync_queue
WHERE user_id = ANY ($1)
