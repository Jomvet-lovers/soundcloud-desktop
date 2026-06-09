SELECT COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '24 hours')::int8 AS "active_24h!", COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '7 days')::int8 AS "active_7d!", COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '30 days')::int8 AS "active_30d!", COUNT(*) ::int8 AS "total!"
FROM sessions
