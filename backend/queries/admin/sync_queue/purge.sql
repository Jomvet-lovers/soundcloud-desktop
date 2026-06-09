DELETE
FROM sync_queue
WHERE retry_count >= $1
