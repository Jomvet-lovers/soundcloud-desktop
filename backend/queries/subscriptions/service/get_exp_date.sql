SELECT exp_date
FROM subscriptions
WHERE user_urn = ANY ($1)
