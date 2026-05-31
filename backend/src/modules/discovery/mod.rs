//! Catalog discovery on the work::Scheduler substrate: walk every artist on
//! Genius/MB on a freshness cadence (no confidence floor, no lifetime cap) to
//! pull fresh tracks/albums into wanted_tracks. Two lanes isolate MB's 1.1s
//! throttle from the Genius firehose. Wanted-track resolution lives in
//! enrich::wanted_resolver (claim-based over its own substrate).

pub mod catalog;
pub mod interest;

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::config::DiscoveryCfg;
use crate::modules::enrich::ArtistCrawlService;
use crate::modules::work::{self, SchedulerPolicy};

use catalog::{CatalogSource, Lane};

const TICK: Duration = Duration::from_secs(10);
const LEASE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub fn spawn(
    pg: PgPool,
    crawl: Arc<ArtistCrawlService>,
    cfg: &DiscoveryCfg,
    shutdown: CancellationToken,
) {
    if !cfg.enabled {
        info!("discovery disabled by config");
        return;
    }

    let genius = Arc::new(CatalogSource::new(
        pg.clone(),
        crawl.clone(),
        Lane::Genius,
        cfg.recrawl_days,
        cfg.max_fails,
    ));
    work::spawn(
        genius,
        SchedulerPolicy {
            name: "catalog:genius",
            concurrency: cfg.genius_concurrency.max(1),
            batch: cfg.batch.max(1),
            tick: TICK,
            lease_timeout: LEASE_TIMEOUT,
        },
        shutdown.clone(),
    );

    let mb = Arc::new(CatalogSource::new(
        pg.clone(),
        crawl,
        Lane::Mb,
        cfg.recrawl_days,
        cfg.max_fails,
    ));
    work::spawn(
        mb,
        SchedulerPolicy {
            name: "catalog:mb",
            concurrency: cfg.mb_concurrency.max(1),
            batch: cfg.batch.max(1),
            tick: TICK,
            lease_timeout: LEASE_TIMEOUT,
        },
        shutdown.clone(),
    );

    interest::spawn(pg, cfg.interest_interval_sec, shutdown);
}
