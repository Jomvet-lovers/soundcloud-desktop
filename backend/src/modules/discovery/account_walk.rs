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
        let rows = sqlx::query_file!(
            "queries/discovery/account_walk/claim.sql",
            self.walk_days,
            lease_secs,
            batch,
        )
        .fetch_all(&self.pg)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| AccountWalkItem {
                id: r.id,
                name: r.name,
            })
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
        if let Err(e) = self
            .wanted
            .run_for_artist(item.id, POST_WALK_WANTED_MAX)
            .await
        {
            tracing::debug!(artist = %item.id, error = %e, "post-walk wanted resolve failed");
        }
        WorkOutcome::Done
    }

    async fn on_success(&self, item: &AccountWalkItem) -> AppResult<()> {
        sqlx::query_file!(
            "queries/discovery/account_walk/clear_lock_success.sql",
            item.id,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn on_failure(&self, item: &AccountWalkItem, _outcome: &WorkOutcome) -> AppResult<()> {
        sqlx::query_file!(
            "queries/discovery/account_walk/clear_lock_success.sql",
            item.id,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }
}
