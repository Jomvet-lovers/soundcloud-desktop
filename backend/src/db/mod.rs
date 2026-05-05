use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};
use tracing::log::LevelFilter;

use crate::config::AppConfig;

pub async fn connect(cfg: &AppConfig) -> Result<PgPool, sqlx::Error> {
    let opts = PgConnectOptions::from_str(&cfg.database.url)?
        .log_statements(LevelFilter::Debug)
        .log_slow_statements(LevelFilter::Warn, Duration::from_millis(500));

    PgPoolOptions::new()
        .max_connections(cfg.database.pool_max)
        .acquire_timeout(cfg.database.acquire_timeout)
        .idle_timeout(Some(Duration::from_secs(600)))
        .max_lifetime(Some(Duration::from_secs(1800)))
        .test_before_acquire(true)
        .connect_with(opts)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    const LOCK_KEY: i64 = 0x5343445F4D49_i64;

    let mut conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *conn)
        .await?;

    let result = sqlx::migrate!("./migrations").run(&mut *conn).await;

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(LOCK_KEY)
        .execute(&mut *conn)
        .await?;

    result.map_err(|e| sqlx::Error::Migrate(Box::new(e)))
}
