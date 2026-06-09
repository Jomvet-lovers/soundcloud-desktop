SELECT last_error AS "last_error!"
FROM sync_queue
WHERE last_error IS NOT NULL
ORDER BY next_run_at DESC LIMIT 10
