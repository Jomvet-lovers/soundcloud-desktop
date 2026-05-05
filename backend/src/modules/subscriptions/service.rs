use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::error::AppResult;

const SNAPSHOT_FILE: &str = "subscriptions.json";
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Subscription {
    pub user_urn: String,
    pub exp_date: i64,
}

pub struct SubscriptionsService {
    pg: PgPool,
    snapshot_dir: PathBuf,
    always_premium: bool,
}

impl SubscriptionsService {
    pub fn new(pg: PgPool, snapshot_dir: String, always_premium: bool) -> Arc<Self> {
        Arc::new(Self {
            pg,
            snapshot_dir: PathBuf::from(snapshot_dir),
            always_premium,
        })
    }

    pub async fn is_premium(&self, user_urn: &str) -> AppResult<bool> {
        if self.always_premium {
            return Ok(true);
        }
        let now = chrono::Utc::now().timestamp();
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT exp_date FROM subscriptions WHERE user_urn = $1")
                .bind(user_urn)
                .fetch_optional(&self.pg)
                .await?;
        Ok(row.map(|(exp,)| exp > now).unwrap_or(false))
    }

    pub async fn list(&self) -> AppResult<Vec<Subscription>> {
        let rows: Vec<Subscription> =
            sqlx::query_as("SELECT user_urn, exp_date FROM subscriptions ORDER BY exp_date DESC")
                .fetch_all(&self.pg)
                .await?;
        Ok(rows)
    }

    pub async fn upsert(self: &Arc<Self>, user_urn: &str, exp_date: i64) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO subscriptions (user_urn, exp_date) VALUES ($1, $2) \
             ON CONFLICT (user_urn) DO UPDATE SET exp_date = EXCLUDED.exp_date",
        )
        .bind(user_urn)
        .bind(exp_date)
        .execute(&self.pg)
        .await?;
        let svc = self.clone();
        tokio::spawn(async move {
            if let Err(e) = svc.export_snapshot().await {
                warn!(error = %e, "snapshot export failed");
            }
        });
        Ok(())
    }

    pub async fn remove(self: &Arc<Self>, user_urn: &str) -> AppResult<u64> {
        let result = sqlx::query("DELETE FROM subscriptions WHERE user_urn = $1")
            .bind(user_urn)
            .execute(&self.pg)
            .await?;
        let n = result.rows_affected();
        if n > 0 {
            let svc = self.clone();
            tokio::spawn(async move {
                if let Err(e) = svc.export_snapshot().await {
                    warn!(error = %e, "snapshot export failed");
                }
            });
        }
        Ok(n)
    }

    pub async fn restore_from_snapshot(&self) -> AppResult<()> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*)::int8 FROM subscriptions")
            .fetch_one(&self.pg)
            .await?;
        if count.0 > 0 {
            info!(
                n = count.0,
                "Subscriptions table populated, skipping restore"
            );
            return Ok(());
        }
        let path = self.snapshot_dir.join(SNAPSHOT_FILE);
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            info!(?path, "No snapshot file found, starting fresh");
            return Ok(());
        }
        let raw = match tokio::fs::read_to_string(&path).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Snapshot read failed");
                return Ok(());
            }
        };
        let subs: Vec<Subscription> = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "Snapshot parse failed");
                return Ok(());
            }
        };
        if subs.is_empty() {
            return Ok(());
        }
        let urns: Vec<String> = subs.iter().map(|s| s.user_urn.clone()).collect();
        let exps: Vec<i64> = subs.iter().map(|s| s.exp_date).collect();
        sqlx::query(
            "INSERT INTO subscriptions (user_urn, exp_date) \
             SELECT * FROM UNNEST($1::text[], $2::int8[]) \
             ON CONFLICT (user_urn) DO UPDATE SET exp_date = EXCLUDED.exp_date",
        )
        .bind(&urns)
        .bind(&exps)
        .execute(&self.pg)
        .await?;
        info!(count = subs.len(), "Restored subscriptions from snapshot");
        Ok(())
    }

    pub async fn export_snapshot(&self) -> AppResult<()> {
        let subs: Vec<Subscription> =
            sqlx::query_as("SELECT user_urn, exp_date FROM subscriptions")
                .fetch_all(&self.pg)
                .await?;
        if let Err(e) = tokio::fs::create_dir_all(&self.snapshot_dir).await {
            warn!(dir = ?self.snapshot_dir, error = %e, "snapshot mkdir failed");
            return Ok(());
        }
        let path = self.snapshot_dir.join(SNAPSHOT_FILE);
        let body = serde_json::to_string_pretty(&subs)
            .map_err(|e| crate::error::AppError::internal(format!("snapshot encode: {e}")))?;
        if let Err(e) = tokio::fs::write(&path, body).await {
            warn!(?path, error = %e, "snapshot write failed");
        } else {
            debug!(?path, count = subs.len(), "snapshot exported");
        }
        Ok(())
    }

    pub fn spawn_snapshot_loop(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(SNAPSHOT_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = svc.export_snapshot().await {
                            warn!(error = %e, "scheduled snapshot failed");
                        }
                    }
                }
            }
        });
    }
}
