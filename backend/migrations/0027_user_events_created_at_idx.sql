-- Поддерживает time-window сканы по user_events: collab trainer build_sessions
-- (WHERE created_at >= since) и crawl-активити агрегацию. Раньше был только
-- композит (sc_user_id, event_type, created_at) — голый created_at фильтр падал
-- в seq scan по растущей таблице.
CREATE INDEX IF NOT EXISTS user_events_created_at_idx ON user_events (created_at);
