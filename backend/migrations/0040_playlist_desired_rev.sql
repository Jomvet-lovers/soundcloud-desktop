-- Local-first owned-плейлисты: desired vs synced ревизия membership.
-- desired_rev > synced_rev  ⇔  есть pending локальная правка (наше побеждает;
-- SC-refresh не перетирает desired-state). Существующие строки получают 0==0
-- («в синке с SC») — корректно, бэкфилл данных не нужен.
--
-- DEFAULT 0 — константа ⇒ fast ADD COLUMN без переписывания таблицы. Частичный
-- индекс на старте пуст (у всех desired_rev=synced_rev) ⇒ строится мгновенно.
ALTER TABLE playlists
    ADD COLUMN IF NOT EXISTS desired_rev bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS synced_rev bigint NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS playlists_pending_sync_idx
    ON playlists (urn) WHERE desired_rev > synced_rev;
