use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::enrich::{ArtistCrawlService, WantedResolverService};
use crate::modules::work::{next_run_after, WorkOutcome, WorkSource};

const POST_CRAWL_WANTED_MAX: i64 = 500;

const BACKOFF_BASE: Duration = Duration::from_secs(60 * 60);
const BACKOFF_CAP: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Clone, Copy)]
pub enum Lane {
    /// Wide proxy-parallel lane over artists with a genius_artist_id (crawls
    /// their MB too if they also have an mb_artist_id — crawl_one branches).
    Genius,
    /// Serialized lane (concurrency 1) for MB-only artists, isolating the 1.1s
    /// MusicBrainz throttle from the Genius firehose.
    Mb,
}

pub struct CatalogItem {
    pub id: Uuid,
    pub mb_id: Option<String>,
    pub genius_id: Option<String>,
    pub sc_user_id: Option<String>,
    pub mb_off: i32,
    pub genius_off: i32,
    pub fail_count: i16,
}

/// Continuous full-catalog walker over `artists`. No confidence floor, no
/// lifetime-attempt cap — eligibility is `merged_into IS NULL AND NOT crawl_dead
/// AND <lane>_next_run_at <= now()`, so every artist with an external id is
/// reachable on a freshness cadence. run() reuses ArtistCrawlService::crawl_one.
pub struct CatalogSource {
    pg: PgPool,
    crawl: Arc<ArtistCrawlService>,
    wanted: Option<Arc<WantedResolverService>>,
    lane: Lane,
    recrawl_days: i64,
    max_fails: i16,
}

impl CatalogSource {
    pub fn new(
        pg: PgPool,
        crawl: Arc<ArtistCrawlService>,
        wanted: Option<Arc<WantedResolverService>>,
        lane: Lane,
        recrawl_days: i64,
        max_fails: i16,
    ) -> Self {
        Self {
            pg,
            crawl,
            wanted,
            lane,
            recrawl_days,
            max_fails,
        }
    }
}

impl WorkSource for CatalogSource {
    type Item = CatalogItem;

    fn name(&self) -> &'static str {
        match self.lane {
            Lane::Genius => "catalog:genius",
            Lane::Mb => "catalog:mb",
        }
    }

    async fn claim(&self, batch: i64, lease_timeout: Duration) -> AppResult<Vec<CatalogItem>> {
        let lease_secs = lease_timeout.as_secs() as f64;
        let items = match self.lane {
            Lane::Genius => sqlx::query_file!(
                "queries/discovery/catalog/claim_genius.sql",
                lease_secs,
                batch
            )
            .fetch_all(&self.pg)
            .await?
            .into_iter()
            .map(|r| CatalogItem {
                id: r.id,
                mb_id: r.mb_artist_id,
                genius_id: r.genius_artist_id,
                sc_user_id: r.sc_user_id,
                mb_off: r.mb_crawl_offset,
                genius_off: r.genius_crawl_offset,
                fail_count: r.crawl_fail_count,
            })
            .collect(),
            Lane::Mb => {
                sqlx::query_file!("queries/discovery/catalog/claim_mb.sql", lease_secs, batch)
                    .fetch_all(&self.pg)
                    .await?
                    .into_iter()
                    .map(|r| CatalogItem {
                        id: r.id,
                        mb_id: r.mb_artist_id,
                        genius_id: r.genius_artist_id,
                        sc_user_id: r.sc_user_id,
                        mb_off: r.mb_crawl_offset,
                        genius_off: r.genius_crawl_offset,
                        fail_count: r.crawl_fail_count,
                    })
                    .collect()
            }
        };
        Ok(items)
    }

    async fn claim_one(
        &self,
        _key: &str,
        _lease_timeout: Duration,
    ) -> AppResult<Option<CatalogItem>> {
        Ok(None)
    }

    async fn run(&self, item: &CatalogItem) -> WorkOutcome {
        match self
            .crawl
            .crawl_one(
                item.id,
                item.mb_id.as_deref(),
                item.genius_id.as_deref(),
                item.sc_user_id.as_deref(),
                item.mb_off as u32,
                item.genius_off as u32,
            )
            .await
        {
            Ok(()) => {
                if item.sc_user_id.is_some() {
                    if let Some(resolver) = &self.wanted {
                        if let Err(e) = resolver
                            .run_for_artist(item.id, POST_CRAWL_WANTED_MAX)
                            .await
                        {
                            tracing::debug!(artist = %item.id, error = %e, "post-crawl wanted resolve failed");
                        }
                    }
                }
                WorkOutcome::Done
            }
            Err(e) => WorkOutcome::Failed {
                error: e.to_string(),
            },
        }
    }

    async fn on_success(&self, item: &CatalogItem) -> AppResult<()> {
        // Genius lane crawled the artist's MB too (crawl_one branches), so it
        // refreshes both cursors; MB lane only owns MB-only artists.
        match self.lane {
            Lane::Genius => {
                sqlx::query_file!(
                    "queries/discovery/catalog/on_success_genius.sql",
                    item.id,
                    self.recrawl_days as f64
                )
                .execute(&self.pg)
                .await?;
            }
            Lane::Mb => {
                sqlx::query_file!(
                    "queries/discovery/catalog/on_success_mb.sql",
                    item.id,
                    self.recrawl_days as f64
                )
                .execute(&self.pg)
                .await?;
            }
        }
        Ok(())
    }

    async fn on_failure(&self, item: &CatalogItem, _outcome: &WorkOutcome) -> AppResult<()> {
        let fail_count = item.fail_count + 1;
        if fail_count >= self.max_fails {
            match self.lane {
                Lane::Genius => {
                    sqlx::query_file!(
                        "queries/discovery/catalog/fail_dead_genius.sql",
                        item.id,
                        fail_count
                    )
                    .execute(&self.pg)
                    .await?;
                }
                Lane::Mb => {
                    sqlx::query_file!(
                        "queries/discovery/catalog/fail_dead_mb.sql",
                        item.id,
                        fail_count
                    )
                    .execute(&self.pg)
                    .await?;
                }
            }
        } else {
            let next = next_run_after(fail_count as i32, BACKOFF_BASE, BACKOFF_CAP);
            match self.lane {
                Lane::Genius => {
                    sqlx::query_file!(
                        "queries/discovery/catalog/fail_backoff_genius.sql",
                        item.id,
                        fail_count,
                        next
                    )
                    .execute(&self.pg)
                    .await?;
                }
                Lane::Mb => {
                    sqlx::query_file!(
                        "queries/discovery/catalog/fail_backoff_mb.sql",
                        item.id,
                        fail_count,
                        next
                    )
                    .execute(&self.pg)
                    .await?;
                }
            }
        }
        Ok(())
    }
}
