//! Standalone migration runner — discrete deploy step.
//!
//! Run this BEFORE starting the app (with `MIGRATE_ON_BOOT=false` set on the app),
//! so a failing migration fails the deploy step without taking down the currently
//! serving instance. Reuses the same advisory lock as the in-app runner, so a boot
//! migrate and this bin never race.

use std::time::Duration;

use sqlx::postgres::PgPoolOptions;

/// Must stay in sync with `db::advisory_locks::MIGRATIONS`.
const MIGRATIONS_LOCK: i64 = 0x5343_445F_4D49;

#[tokio::main]
async fn main() {
    let url = match std::env::var("DATABASE_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("migrate: DATABASE_URL must be set");
            std::process::exit(1);
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect(&url)
        .await
        .unwrap_or_else(|e| {
            eprintln!("migrate: connect failed: {e}");
            std::process::exit(1);
        });

    let mut conn = pool.acquire().await.unwrap_or_else(|e| {
        eprintln!("migrate: acquire failed: {e}");
        std::process::exit(1);
    });

    if let Err(e) = sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(MIGRATIONS_LOCK)
        .execute(&mut *conn)
        .await
    {
        eprintln!("migrate: advisory lock failed: {e}");
        std::process::exit(1);
    }

    let result = sqlx::migrate!("./migrations").run(&mut *conn).await;

    // Best-effort unlock; the session ending releases it anyway.
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(MIGRATIONS_LOCK)
        .execute(&mut *conn)
        .await;

    match result {
        Ok(()) => println!("migrate: migrations applied"),
        Err(e) => {
            eprintln!("migrate: failed: {e}");
            std::process::exit(1);
        }
    }
}
