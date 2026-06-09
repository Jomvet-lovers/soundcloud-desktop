UPDATE sync_queue
SET locked_at = NULL
WHERE id = ANY ($1)
