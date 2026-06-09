INSERT INTO cluster_bandit_stats (sc_user_id, cluster_id, shows, updated_at)
SELECT $1, c, s, NOW()
FROM UNNEST($2::text[], $3::bigint[]) AS t(c, s) ON CONFLICT (sc_user_id, cluster_id)
DO
UPDATE SET shows = cluster_bandit_stats.shows + EXCLUDED.shows,
    updated_at = NOW()
