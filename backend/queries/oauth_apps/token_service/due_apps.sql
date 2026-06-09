SELECT a.id
FROM oauth_apps a
         LEFT JOIN oauth_app_tokens t ON t.oauth_app_id = a.id
WHERE a.id = ANY ($1)
  AND a.active = true
  AND (t.oauth_app_id IS NULL
    OR (t.expires_at < now() + make_interval(secs => $2)
        AND (t.refresh_attempts < $3
            OR t.refreshed_at < now() - make_interval(secs => $4))))
