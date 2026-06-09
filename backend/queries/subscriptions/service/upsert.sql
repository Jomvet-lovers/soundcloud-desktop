INSERT INTO subscriptions (user_urn, exp_date)
VALUES ($1, $2) ON CONFLICT (user_urn) DO
UPDATE SET exp_date = EXCLUDED.exp_date
