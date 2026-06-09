UPDATE link_requests
SET status = 'failed',
    error  = $2
WHERE id = $1
