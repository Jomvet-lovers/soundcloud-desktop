SELECT a.id,
       a.name,
       a.client_id,
       a.active,
       a.last_used_at,
       COUNT(s.id)::int8 AS "sessions_total!", COUNT(s.id) FILTER (WHERE s.expires_at > (now() at time zone 'utc'))::int8 AS "sessions_active!", COUNT(s.id) FILTER (WHERE s.expires_at <= (now() at time zone 'utc'))::int8 AS "sessions_expired!"
FROM oauth_apps a
         LEFT JOIN sessions s ON s.oauth_app_id = a.id::text
GROUP BY a.id, a.name, a.client_id, a.active, a.last_used_at
ORDER BY "sessions_total!" DESC, a.name ASC
