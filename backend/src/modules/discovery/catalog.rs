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

/// Сырой ряд lease-выборки artists для краула (см. `claim`).
type CrawlSqlRow = (
    Uuid,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
    i32,
    i16,
);

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
        let lease_secs = lease_timeout.as_secs() as i64;
        let sql = match self.lane {
            Lane::Genius => {
                "WITH picked AS (
                     SELECT id FROM artists
                     WHERE merged_into IS NULL AND NOT crawl_dead
                       AND genius_artist_id IS NOT NULL
                       AND genius_next_run_at <= now()
                       AND (genius_locked_at IS NULL
                            OR genius_locked_at < now() - ($1 * interval '1 second'))
                     ORDER BY genius_next_run_at
                     LIMIT $2 FOR UPDATE SKIP LOCKED
                 )
                 UPDATE artists a SET genius_locked_at = now()
                 FROM picked WHERE a.id = picked.id
                 RETURNING a.id, a.mb_artist_id, a.genius_artist_id, a.sc_user_id,
                           a.mb_crawl_offset, a.genius_crawl_offset, a.crawl_fail_count"
            }
            Lane::Mb => {
                "WITH picked AS (
                     SELECT id FROM artists
                     WHERE merged_into IS NULL AND NOT crawl_dead
                       AND mb_artist_id IS NOT NULL AND genius_artist_id IS NULL
                       AND mb_next_run_at <= now()
                       AND (mb_locked_at IS NULL
                            OR mb_locked_at < now() - ($1 * interval '1 second'))
                     ORDER BY mb_next_run_at
                     LIMIT $2 FOR UPDATE SKIP LOCKED
                 )
                 UPDATE artists a SET mb_locked_at = now()
                 FROM picked WHERE a.id = picked.id
                 RETURNING a.id, a.mb_artist_id, a.genius_artist_id, a.sc_user_id,
                           a.mb_crawl_offset, a.genius_crawl_offset, a.crawl_fail_count"
            }
        };
        let rows: Vec<CrawlSqlRow> = sqlx::query_as(sql)
            .bind(lease_secs)
            .bind(batch)
            .fetch_all(&self.pg)
            .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, mb_id, genius_id, sc_user_id, mb_off, genius_off, fail_count)| CatalogItem {
                    id,
                    mb_id,
                    genius_id,
                    sc_user_id,
                    mb_off,
                    genius_off,
                    fail_count,
                },
            )
            .collect())
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
                        if let Err(e) = resolver.run_for_artist(item.id, POST_CRAWL_WANTED_MAX).await
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
        let sql = match self.lane {
            Lane::Genius => {
                "UPDATE artists
                 SET genius_crawled_at = now(),
                     genius_next_run_at = now() + ($2 * interval '1 day'),
                     genius_locked_at = NULL,
                     mb_crawled_at = CASE WHEN mb_artist_id IS NOT NULL THEN now() ELSE mb_crawled_at END,
                     mb_next_run_at = CASE WHEN mb_artist_id IS NOT NULL
                                          THEN now() + ($2 * interval '1 day') ELSE mb_next_run_at END,
                     crawl_fail_count = 0
                 WHERE id = $1"
            }
            Lane::Mb => {
                "UPDATE artists
                 SET mb_crawled_at = now(),
                     mb_next_run_at = now() + ($2 * interval '1 day'),
                     mb_locked_at = NULL,
                     crawl_fail_count = 0
                 WHERE id = $1"
            }
        };
        sqlx::query(sql)
            .bind(item.id)
            .bind(self.recrawl_days)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn on_failure(&self, item: &CatalogItem, _outcome: &WorkOutcome) -> AppResult<()> {
        let fail_count = item.fail_count + 1;
        if fail_count >= self.max_fails {
            let sql = match self.lane {
                Lane::Genius => {
                    "UPDATE artists SET crawl_dead = true, crawl_fail_count = $2,
                         genius_locked_at = NULL WHERE id = $1"
                }
                Lane::Mb => {
                    "UPDATE artists SET crawl_dead = true, crawl_fail_count = $2,
                         mb_locked_at = NULL WHERE id = $1"
                }
            };
            sqlx::query(sql)
                .bind(item.id)
                .bind(fail_count)
                .execute(&self.pg)
                .await?;
        } else {
            let next = next_run_after(fail_count as i32, BACKOFF_BASE, BACKOFF_CAP);
            let sql = match self.lane {
                Lane::Genius => {
                    "UPDATE artists SET genius_next_run_at = $3, crawl_fail_count = $2,
                         genius_locked_at = NULL WHERE id = $1"
                }
                Lane::Mb => {
                    "UPDATE artists SET mb_next_run_at = $3, crawl_fail_count = $2,
                         mb_locked_at = NULL WHERE id = $1"
                }
            };
            sqlx::query(sql)
                .bind(item.id)
                .bind(fail_count)
                .bind(next)
                .execute(&self.pg)
                .await?;
        }
        Ok(())
    }
}
