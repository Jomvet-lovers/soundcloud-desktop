DELETE
FROM login_requests
WHERE expires_at < $1
