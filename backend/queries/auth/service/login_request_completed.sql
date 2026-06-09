UPDATE login_requests
SET status            = 'completed',
    step              = NULL,
    result_session_id = $2,
    username          = $3,
    profile_ok        = $4
WHERE id = $1
