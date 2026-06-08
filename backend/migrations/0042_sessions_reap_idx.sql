CREATE INDEX IF NOT EXISTS sessions_reap_idx ON sessions (expires_at, updated_at);
