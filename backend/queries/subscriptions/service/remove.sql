DELETE
FROM subscriptions
WHERE user_urn = ANY ($1)
