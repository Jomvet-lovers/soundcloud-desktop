DELETE
FROM sessions
WHERE expires_at <= now()
  AND updated_at < now() - interval '7 days'
