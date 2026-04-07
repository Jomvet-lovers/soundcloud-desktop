use rusqlite::Connection;
use std::sync::Mutex;
use tracing::info;

pub struct SqliteDb {
    conn: Mutex<Connection>,
}

#[derive(Debug, serde::Serialize)]
pub struct Subscription {
    pub user_urn: String,
    pub exp_date: i64,
}

impl SqliteDb {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 10000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS subscriptions (
                user_urn TEXT PRIMARY KEY,
                exp_date INTEGER NOT NULL
            )",
            [],
        )?;

        info!("SQLite opened: {path}");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Check if user has an active (non-expired) subscription
    pub fn is_premium(&self, user_urn: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let mut stmt =
            conn.prepare("SELECT 1 FROM subscriptions WHERE user_urn = ?1 AND exp_date > ?2")?;
        Ok(stmt.exists(rusqlite::params![user_urn, now])?)
    }

    /// List all subscriptions
    pub fn list_subscriptions(&self) -> Result<Vec<Subscription>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT user_urn, exp_date FROM subscriptions ORDER BY exp_date DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Subscription {
                user_urn: row.get(0)?,
                exp_date: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// Insert or update subscription
    pub fn upsert_subscription(
        &self,
        user_urn: &str,
        exp_date: i64,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO subscriptions (user_urn, exp_date) VALUES (?1, ?2)
             ON CONFLICT(user_urn) DO UPDATE SET exp_date = ?2",
            rusqlite::params![user_urn, exp_date],
        )?;
        Ok(())
    }

    /// Delete subscription
    pub fn delete_subscription(&self, user_urn: &str) -> Result<bool, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute(
            "DELETE FROM subscriptions WHERE user_urn = ?1",
            rusqlite::params![user_urn],
        )?;
        Ok(affected > 0)
    }
}
