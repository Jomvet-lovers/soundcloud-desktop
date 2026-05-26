//! Stable, named IDs для `pg_advisory_lock`. Все ID — статика; ни один не
//! пересекается с другим. Используется для координации singleton-cron'ов
//! между инстансами бекенда.
//!
//! Для per-entity tx-level lock'ов (replace-операции типа `playlist_tracks`
//! reorder) используем `pg_advisory_xact_lock(hashtext($1))` с осмысленным
//! строковым ключом — см. callers.

/// Lock на запуск sqlx-миграций: только один процесс держит его на старте.
pub const MIGRATIONS: i64 = 0x5343445F4D49;
/// Lock на тик `wanted_resolver` — singleton кросс-инстансов.
pub const WANTED_RESOLVER_TICK: i64 = 0x0007_7AED_5E50_4EE0;
