-- Sync-очередь: give-up больше НЕ удаляет намерение юзера. После MAX_RETRIES
-- строку паркуем (dead=true) — она остаётся durable, видна в /admin/sync-queue
-- и в sync-badge'е, и реконсилируется heal-свипом из desired-state.
-- DEFAULT false — константа ⇒ fast ADD COLUMN.
ALTER TABLE sync_queue
    ADD COLUMN IF NOT EXISTS dead boolean NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS failed_at timestamptz;

-- Живой claim сканирует только не-dead строки.
CREATE INDEX IF NOT EXISTS sync_queue_live_pickup_idx
    ON sync_queue (next_run_at, created_at) WHERE dead = false;

-- Поддержка per-(user,target) сериализации (anti-join по живым lease'ам).
CREATE INDEX IF NOT EXISTS sync_queue_user_target_idx
    ON sync_queue (user_id, target_urn);
