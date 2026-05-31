use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::AppResult;
use crate::modules::work::{next_run_after, WorkOutcome, WorkSource};

use super::service::EnrichService;

const BACKOFF_BASE: Duration = Duration::from_secs(5 * 60);
const BACKOFF_CAP: Duration = Duration::from_secs(6 * 60 * 60);

pub struct EnrichItem {
    pub id: Uuid,
    pub sc_track_id: String,
    /// Post-claim attempt count (already incremented), drives backoff/terminal.
    pub attempts: i16,
}

/// Enrich as a `WorkSource` over `tracks`. claim leases + increments attempts in
/// one statement, ordered by index_priority so user-relevant work jumps the
/// 2.5M backlog. run() = the unchanged resolver cascade via EnrichService.
pub struct EnrichSource {
    pg: PgPool,
    svc: Arc<EnrichService>,
    max_attempts: i16,
}

impl EnrichSource {
    pub fn new(pg: PgPool, svc: Arc<EnrichService>, max_attempts: i16) -> Self {
        Self {
            pg,
            svc,
            max_attempts,
        }
    }
}

impl WorkSource for EnrichSource {
    type Item = EnrichItem;

    fn name(&self) -> &'static str {
        "enrich"
    }

    async fn claim(&self, batch: i64, lease_timeout: Duration) -> AppResult<Vec<EnrichItem>> {
        let lease_secs = lease_timeout.as_secs() as i64;
        let rows: Vec<(Uuid, String, i16)> = sqlx::query_as(
            "WITH picked AS (
                 SELECT id FROM tracks
                 WHERE enrich_state IN ('pending', 'failed')
                   AND enrich_next_run_at <= now()
                   AND (enrich_locked_at IS NULL
                        OR enrich_locked_at < now() - ($1 * interval '1 second'))
                   AND enrich_attempts < $2
                 ORDER BY index_priority, enrich_next_run_at
                 LIMIT $3
                 FOR UPDATE SKIP LOCKED
             )
             UPDATE tracks t
             SET enrich_locked_at = now(), enrich_attempts = t.enrich_attempts + 1
             FROM picked
             WHERE t.id = picked.id
             RETURNING t.id, t.sc_track_id, t.enrich_attempts",
        )
            .bind(lease_secs)
            .bind(self.max_attempts)
            .bind(batch)
            .fetch_all(&self.pg)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, sc_track_id, attempts)| EnrichItem {
                id,
                sc_track_id,
                attempts,
            })
            .collect())
    }

    async fn claim_one(
        &self,
        key: &str,
        lease_timeout: Duration,
    ) -> AppResult<Option<EnrichItem>> {
        let Some(sc_id) = normalize_sc_track_id(key) else {
            return Ok(None);
        };
        let lease_secs = lease_timeout.as_secs() as i64;
        let row: Option<(Uuid, String, i16)> = sqlx::query_as(
            "WITH picked AS (
                 SELECT id FROM tracks
                 WHERE sc_track_id = $3
                   AND (enrich_locked_at IS NULL
                        OR enrich_locked_at < now() - ($1 * interval '1 second'))
                   AND enrich_attempts < $2
                   AND ( enrich_state IN ('pending', 'failed')
                         OR (enrich_state = 'done'
                             AND (enriched_at IS NULL
                                  OR enriched_at < now() - interval '24 hours')) )
                 LIMIT 1
                 FOR UPDATE SKIP LOCKED
             )
             UPDATE tracks t
             SET enrich_locked_at = now(), enrich_attempts = t.enrich_attempts + 1
             FROM picked
             WHERE t.id = picked.id
             RETURNING t.id, t.sc_track_id, t.enrich_attempts",
        )
            .bind(lease_secs)
            .bind(self.max_attempts)
            .bind(&sc_id)
            .fetch_optional(&self.pg)
            .await?;
        Ok(row.map(|(id, sc_track_id, attempts)| EnrichItem {
            id,
            sc_track_id,
            attempts,
        }))
    }

    async fn run(&self, item: &EnrichItem) -> WorkOutcome {
        match self.svc.process_track(&item.sc_track_id).await {
            Ok(()) => WorkOutcome::Done,
            Err(e) => WorkOutcome::Failed {
                error: e.to_string(),
            },
        }
    }

    async fn on_success(&self, _item: &EnrichItem) -> AppResult<()> {
        // persist::apply already wrote done + cleared lease + reset attempts.
        Ok(())
    }

    async fn on_failure(&self, item: &EnrichItem, outcome: &WorkOutcome) -> AppResult<()> {
        match outcome {
            WorkOutcome::ExternalBlocked { backoff } => {
                let next = Utc::now()
                    + chrono::Duration::from_std(*backoff)
                    .unwrap_or_else(|_| chrono::Duration::minutes(10));
                sqlx::query(
                    "UPDATE tracks
                     SET enrich_locked_at = NULL,
                         enrich_attempts = GREATEST(0, enrich_attempts - 1),
                         enrich_next_run_at = $2
                     WHERE id = $1",
                )
                    .bind(item.id)
                    .bind(next)
                    .execute(&self.pg)
                    .await?;
            }
            _ => {
                let err: Option<String> = match outcome {
                    WorkOutcome::Failed { error } => Some(error.chars().take(300).collect()),
                    _ => None,
                };
                if item.attempts >= self.max_attempts {
                    sqlx::query(
                        "UPDATE tracks
                         SET enrich_state = 'dead', enrich_locked_at = NULL,
                             enrich_error = $2, enriched_at = now()
                         WHERE id = $1",
                    )
                        .bind(item.id)
                        .bind(err.as_deref())
                        .execute(&self.pg)
                        .await?;
                } else {
                    let next = next_run_after(item.attempts as i32, BACKOFF_BASE, BACKOFF_CAP);
                    sqlx::query(
                        "UPDATE tracks
                         SET enrich_state = 'failed', enrich_locked_at = NULL,
                             enrich_next_run_at = $2, enrich_error = $3
                         WHERE id = $1",
                    )
                        .bind(item.id)
                        .bind(next)
                        .bind(err.as_deref())
                        .execute(&self.pg)
                        .await?;
                }
            }
        }
        Ok(())
    }
}
