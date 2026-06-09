SELECT action_type, COUNT(*) ::int8 AS "count!"
FROM sync_queue
GROUP BY action_type
ORDER BY COUNT(*) DESC
