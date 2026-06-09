SELECT COUNT(*) AS "count!"
FROM listening_history
WHERE soundcloud_user_id = ANY ($1)
