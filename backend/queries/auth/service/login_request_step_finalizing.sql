UPDATE login_requests
SET step = 'finalizing'
WHERE id = $1
