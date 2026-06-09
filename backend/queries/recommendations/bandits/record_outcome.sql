INSERT INTO cluster_bandit_stats (sc_user_id, cluster_id, clicks, completes, updated_at)
VALUES ($1, $2, $3, $4, NOW()) ON CONFLICT (sc_user_id, cluster_id)
DO
UPDATE SET clicks = cluster_bandit_stats.clicks + EXCLUDED.clicks,
    completes = cluster_bandit_stats.completes + EXCLUDED.completes,
    updated_at = NOW()
