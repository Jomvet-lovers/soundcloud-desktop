use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::bus::nats::NatsService;
use crate::qdrant::QdrantService;

use super::quality_scorer;
use super::service::RecommendationsService;
use super::trainer;

const DEFAULT_CRON_SECS: u64 = 6 * 3600;
const QUALITY_BACKFILL_SECS: u64 = 600;

pub fn spawn_cron_loops(
    service: Arc<RecommendationsService>,
    nats: Arc<NatsService>,
    qdrant: Arc<QdrantService>,
    pg: PgPool,
    shutdown: CancellationToken,
) {
    let trainer_secs = std::env::var("RECS_TRAINER_CRON_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n >= 60)
        .unwrap_or(DEFAULT_CRON_SECS);

    let quality_secs = std::env::var("RECS_QUALITY_BACKFILL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n >= 60)
        .unwrap_or(QUALITY_BACKFILL_SECS);

    info!(
        trainer_secs,
        quality_secs, "recommendations: spawning cron loops"
    );

    {
        let pg = pg.clone();
        let nats = nats.clone();
        let qdrant = qdrant.clone();
        let service_inner = service.clone();
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            tick_with_shutdown(Duration::from_secs(trainer_secs), shutdown_clone, |_| {
                let pg = pg.clone();
                let nats = nats.clone();
                let qdrant = qdrant.clone();
                let svc = service_inner.clone();
                async move {
                    if let Err(e) = trainer::kick_off_two_tower(&pg, nats.clone()).await {
                        warn!(error = %e, "trainer cron: two_tower failed");
                    }
                    if let Err(e) = trainer::kick_off_sequential(&pg, qdrant.clone(), nats.clone()).await {
                        warn!(error = %e, "trainer cron: sequential failed");
                    }
                    if let Err(e) = trainer::kick_off_quality(svc, nats.clone()).await {
                        warn!(error = %e, "trainer cron: quality failed");
                    }
                }
            })
            .await;
        });
    }

    {
        let service_inner = service.clone();
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            tick_with_shutdown(Duration::from_secs(quality_secs), shutdown_clone, |_| {
                let svc = service_inner.clone();
                async move {
                    match quality_scorer::backfill_missing_scores(svc).await {
                        Ok(0) => {}
                        Ok(n) => info!(n, "quality_scorer: backfilled"),
                        Err(e) => warn!(error = %e, "quality_scorer cron failed"),
                    }
                }
            })
            .await;
        });
    }
}

async fn tick_with_shutdown<F, Fut>(period: Duration, shutdown: CancellationToken, mut tick: F)
where
    F: FnMut(()) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = interval.tick() => {
                tick(()).await;
            }
        }
    }
}
