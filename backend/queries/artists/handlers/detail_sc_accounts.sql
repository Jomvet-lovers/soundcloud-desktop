SELECT sc_user_id, role, source, verified
FROM artist_sc_accounts
WHERE artist_id = $1
  AND role IN ('main', 'demo')
ORDER BY verified DESC,
         CASE role WHEN 'main' THEN 0 WHEN 'demo' THEN 1 ELSE 2 END,
         CASE source
             WHEN 'manual' THEN 0
             WHEN 'auto_match' THEN 1
             WHEN 'mb_resolve' THEN 2
             ELSE 3 END,
         sc_user_id
