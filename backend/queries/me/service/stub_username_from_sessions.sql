SELECT username AS "username!"
FROM sessions
WHERE soundcloud_user_id = ANY ($1)
  AND username IS NOT NULL
ORDER BY updated_at DESC LIMIT 1
