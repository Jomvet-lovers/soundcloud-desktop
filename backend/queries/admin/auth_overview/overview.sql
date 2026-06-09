SELECT COUNT(*)::int8 AS "total!", COUNT(*) FILTER (WHERE expires_at > (now() at time zone 'utc'))::int8 AS "valid!", COUNT(*) FILTER (WHERE expires_at <= (now() at time zone 'utc'))::int8 AS "expired!", COUNT(*) FILTER (WHERE expires_at > (now() at time zone 'utc')
                     AND expires_at <= (now() at time zone 'utc') + interval '1 hour')::int8 AS "expiring_1h!", COUNT(DISTINCT soundcloud_user_id)::int8 AS "distinct_users!", COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '24 hours')::int8 AS "active_24h!"
FROM sessions
