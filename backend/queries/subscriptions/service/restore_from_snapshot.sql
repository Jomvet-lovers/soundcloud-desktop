INSERT INTO subscriptions (user_urn, exp_date)
SELECT *
FROM UNNEST($1::text[], $2::int8[]) ON CONFLICT (user_urn) DO
UPDATE SET exp_date = EXCLUDED.exp_date
