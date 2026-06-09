-- Pickup для ретрая storage_state='failed' (реап раз в сутки по updated_at).
-- На проде пред-создать CONCURRENTLY до деплоя, миграция no-op-нется.
CREATE INDEX IF NOT EXISTS tracks_storage_failed_retry_idx
    ON tracks (updated_at)
    WHERE storage_state = 'failed';
