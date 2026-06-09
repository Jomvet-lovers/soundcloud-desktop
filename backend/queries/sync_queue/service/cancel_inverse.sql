DELETE
FROM sync_queue
WHERE user_id = $1
  AND action_type = $2
  AND target_urn = $3
  AND locked_at IS NULL
