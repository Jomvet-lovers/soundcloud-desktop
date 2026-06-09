SELECT id
FROM sessions
WHERE soundcloud_user_id = ANY ($1)
ORDER BY updated_at DESC LIMIT 1
