-- Sync-очередь для оптимистично-локальных мутаций пользователя (Phase 2/3):
-- like/unlike/repost/follow/comment/playlist_*. Воркер тикером забирает строки
-- по locked_at IS NULL, проводит SC-вызов, на успехе DELETE, на ошибке снимает
-- lock — попадёт в следующий тик. Дедуп идентичных запросов — через UNIQUE
-- (user_id, action_type, target_urn). До этой миграции жил аналог под именем
-- `pending_actions`, но c семантикой "session-bound outbox" и без дедупа на
-- inverse-операции — переписан целиком.

CREATE TABLE "sync_queue" (
    "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    "user_id" text NOT NULL,
    "action_type" varchar(32) NOT NULL,
    "target_urn" text NOT NULL,
    "payload" jsonb,
    "locked_at" timestamp with time zone,
    "retry_count" integer NOT NULL DEFAULT 0,
    "last_error" text,
    "next_run_at" timestamp with time zone NOT NULL DEFAULT now(),
    "created_at" timestamp with time zone NOT NULL DEFAULT now()
);

CREATE INDEX "sync_queue_pickup_idx"
    ON "sync_queue" ("next_run_at", "locked_at");
CREATE UNIQUE INDEX "sync_queue_target_uq"
    ON "sync_queue" ("user_id", "action_type", "target_urn");

-- Переносим in-flight мутации из старой `pending_actions` в `sync_queue` с
-- маппингом действий на новые имена. `playlist_create` получает nonce-URN
-- ("new:{uuid}") — у него нет ресурса, дедуп через target_urn не работает,
-- но и схлопывать параллельные create'ы нельзя. Репосты в новый sync_queue
-- НЕ переносим: фича вырезана из десктопа, ловить их в воркере некому.
INSERT INTO sync_queue (user_id, action_type, target_urn, payload, created_at)
SELECT
    s.soundcloud_user_id,
    CASE p.action_type
        WHEN 'like'   THEN 'like_track'
        WHEN 'unlike' THEN 'unlike_track'
        ELSE p.action_type
    END,
    CASE
        WHEN p.action_type = 'playlist_create'
            THEN 'new:' || p.id::text
        ELSE p.target_urn
    END,
    p.payload,
    p.created_at
FROM pending_actions p
JOIN sessions s ON s.id::text = p.session_id
WHERE p.status = 'pending'
  AND p.action_type NOT IN ('repost', 'unrepost')
  AND s.soundcloud_user_id IS NOT NULL
ON CONFLICT (user_id, action_type, target_urn) DO NOTHING;

DROP TABLE "pending_actions";
