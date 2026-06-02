use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::enrich::{ArtistAccountWalker, WantedResolverService};
use crate::modules::work::{WorkOutcome, WorkSource};

const POST_WALK_WANTED_MAX: i64 = 500;

pub struct AccountWalkItem {
    pub id: Uuid,
    pub name: String,
}

pub struct AccountWalkSource {
    pg: PgPool,
    walker: Arc<ArtistAccountWalker>,
    wanted: Arc<WantedResolverService>,
    walk_days: i64,
}

impl AccountWalkSource {
    pub fn new(
        pg: PgPool,
        walker: Arc<ArtistAccountWalker>,
        wanted: Arc<WantedResolverService>,
        walk_days: i64,
    ) -> Self {
        Self {
            pg,
            walker,
            wanted,
            walk_days,
        }
    }
}

impl WorkSource for AccountWalkSource {
    type Item = AccountWalkItem;

    fn name(&self) -> &'static str {
        "account_walk"
    }

    async fn claim(&self, batch: i64, lease_timeout: Duration) -> AppResult<Vec<AccountWalkItem>> {
        let lease_secs = lease_timeout.as_secs() as i64;
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "WITH picked AS (
                 SELECT ar.id FROM artists ar
                 WHERE ar.merged_into IS NULL
                   AND (ar.last_account_walk_at IS NULL
                        OR ar.last_account_walk_at < now() - ($1 * interval '1 day'))
                   AND (ar.account_walk_locked_at IS NULL
                        OR ar.account_walk_locked_at < now() - ($2 * interval '1 second'))
                   AND EXISTS (SELECT 1 FROM artist_sc_accounts a
                               WHERE a.artist_id = ar.id AND a.role IN ('main', 'alt', 'demo'))
                 ORDER BY ar.last_account_walk_at NULLS FIRST
                 LIMIT $3
                 FOR UPDATE SKIP LOCKED
             )
             UPDATE artists ar SET account_walk_locked_at = now()
             FROM picked WHERE ar.id = picked.id
             RETURNING ar.id, ar.name",
        )
            .bind(self.walk_days)
            .bind(lease_secs)
            .bind(batch)
            .fetch_all(&self.pg)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name)| AccountWalkItem { id, name })
            .collect())
    }

    async fn claim_one(
        &self,
        _key: &str,
        _lease_timeout: Duration,
    ) -> AppResult<Option<AccountWalkItem>> {
        Ok(None)
    }

    async fn run(&self, item: &AccountWalkItem) -> WorkOutcome {
        if let Err(e) = self.walker.walk_artist(item.id, &item.name).await {
            return WorkOutcome::Failed {
                error: e.to_string(),
            };
        }
        if let Err(e) = self.wanted.run_for_artist(item.id, POST_WALK_WANTED_MAX).await {
            tracing::debug!(artist = %item.id, error = %e, "post-walk wanted resolve failed");
        }
        WorkOutcome::Done
    }

    async fn on_success(&self, item: &AccountWalkItem) -> AppResult<()> {
        sqlx::query(
            "UPDATE artists SET last_account_walk_at = now(), account_walk_locked_at = NULL
             WHERE id = $1",
        )
            .bind(item.id)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn on_failure(&self, item: &AccountWalkItem, _outcome: &WorkOutcome) -> AppResult<()> {
        sqlx::query(
            "UPDATE artists SET last_account_walk_at = now(), account_walk_locked_at = NULL
             WHERE id = $1",
        )
            .bind(item.id)
            .execute(&self.pg)
            .await?;
        Ok(())
    }
}
