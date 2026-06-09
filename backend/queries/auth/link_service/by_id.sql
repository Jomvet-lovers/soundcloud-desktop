SELECT id, mode, source_session_id, target_session_id, status, error, expires_at
FROM link_requests
WHERE id = $1
