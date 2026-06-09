DELETE
FROM sync_queue
WHERE id = $1
  AND locked_at = $2
