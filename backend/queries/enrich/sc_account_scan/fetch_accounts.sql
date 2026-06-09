SELECT sc_user_id, role, source
FROM artist_sc_accounts
WHERE artist_id = $1
ORDER BY verified DESC,
         CASE role
             WHEN 'main' THEN 0
             WHEN 'demo' THEN 1
             WHEN 'alt' THEN 2
             ELSE 3
             END
