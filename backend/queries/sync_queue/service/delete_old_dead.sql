DELETE
FROM sync_queue
WHERE dead = true
  AND failed_at < now() - interval '30 days'
