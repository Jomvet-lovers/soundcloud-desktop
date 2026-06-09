DELETE
FROM link_requests
WHERE expires_at < $1
