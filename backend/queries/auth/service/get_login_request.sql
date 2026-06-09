SELECT id,
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
FROM login_requests
WHERE id = $1
