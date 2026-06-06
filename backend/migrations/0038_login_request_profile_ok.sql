-- Tracks whether the best-effort profile extraction (avatar/username) succeeded
-- during login, so the callback page can mark that step ok/failed. NULL = not
-- reached yet; login completes regardless of the value.
ALTER TABLE login_requests
    ADD COLUMN IF NOT EXISTS profile_ok boolean;
