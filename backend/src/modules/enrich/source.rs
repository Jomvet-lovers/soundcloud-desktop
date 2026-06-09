use std::sync::Arc;
use std::time::Duration;

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
        let lease_secs = lease_timeout.as_secs() as f64;
        let rows = sqlx::query_file!(
            "queries/enrich/source/claim_batch.sql",
            lease_secs,
            self.max_attempts,
            batch
        )
        .fetch_all(&self.pg)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| EnrichItem {
                id: r.id,
                sc_track_id: r.sc_track_id,
                attempts: r.enrich_attempts,
            })
            .collect())
    }

    async fn claim_one(&self, key: &str, lease_timeout: Duration) -> AppResult<Option<EnrichItem>> {
        let Some(sc_id) = normalize_sc_track_id(key) else {
            return Ok(None);
        };
        let lease_secs = lease_timeout.as_secs() as f64;
        let row = sqlx::query_file!(
            "queries/enrich/source/claim_one.sql",
            lease_secs,
            self.max_attempts,
            sc_id
        )
        .fetch_optional(&self.pg)
        .await?;
        Ok(row.map(|r| EnrichItem {
            id: r.id,
            sc_track_id: r.sc_track_id,
            attempts: r.enrich_attempts,
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
        let err: Option<String> = match outcome {
            WorkOutcome::Failed { error } => Some(error.chars().take(300).collect()),
            _ => None,
        };
        if item.attempts >= self.max_attempts {
            sqlx::query_file!(
                "queries/enrich/source/mark_dead.sql",
                item.id,
                err.as_deref()
            )
            .execute(&self.pg)
            .await?;
        } else {
            let next = next_run_after(item.attempts as i32, BACKOFF_BASE, BACKOFF_CAP);
            sqlx::query_file!(
                "queries/enrich/source/mark_failed.sql",
                item.id,
                next,
                err.as_deref()
            )
            .execute(&self.pg)
            .await?;
        }
        Ok(())
    }
}
