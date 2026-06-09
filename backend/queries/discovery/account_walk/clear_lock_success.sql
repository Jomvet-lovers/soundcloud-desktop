UPDATE artists
SET last_account_walk_at   = now(),
    account_walk_locked_at = NULL
WHERE id = $1
