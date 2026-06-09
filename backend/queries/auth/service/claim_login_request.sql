UPDATE login_requests
SET status       = 'processing',
    step         = 'token',
    redirect_url = NULL
WHERE state = $1
  AND status = 'pending' RETURNING id,
          code_verifier,
          oauth_app_id,
          target_session_id,
          status,
          step,
          username,
          result_session_id,
          error,
          retry_count,
          redirect_url,
          profile_ok,
          expires_at
