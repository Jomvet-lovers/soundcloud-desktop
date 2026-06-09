SELECT cluster_id, shows, clicks, completes
FROM cluster_bandit_stats
WHERE sc_user_id = ANY ($1)
