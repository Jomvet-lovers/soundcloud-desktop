SELECT COUNT(*) FILTER (WHERE dead = false)::int8 AS "pending!", COUNT(*) FILTER (WHERE retry_count > 0 OR dead = true)::int8 AS "failed!", COUNT(*) FILTER (WHERE dead = true)::int8 AS "dead!", MIN(created_at) FILTER (WHERE dead = false) AS "oldest_pending_at"
FROM sync_queue
