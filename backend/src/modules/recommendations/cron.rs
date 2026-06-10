use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::bus::nats::NatsService;

use super::quality_scorer;
use super::service::RecommendationsService;
use super::smart_wave::colike;
use super::trainer;

const DEFAULT_CRON_SECS: u64 = 6 * 3600;
const QUALITY_BACKFILL_SECS: u64 = 600;
const COLIKE_REBUILD_SECS: u64 = 6 * 3600;
const WAVE_BUMP_SECS: u64 = 3600;

pub fn spawn_cron_loops(
    service: Arc<RecommendationsService>,
    nats: Arc<NatsService>,
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
        let nats = nats.clone();
        let service_inner = service.clone();
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            tick_with_shutdown(Duration::from_secs(trainer_secs), shutdown_clone, |_| {
                let nats = nats.clone();
                let svc = service_inner.clone();
                async move {
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

    // Ко-лайк рёбра сетки: первый прогон сразу (после деплоя таблица пуста),
    // дальше периодикой. Лайки меняются медленно — 6ч за глаза.
    {
        let service_inner = service.clone();
        let shutdown_clone = shutdown.clone();
        let secs = env_secs("RECS_COLIKE_REBUILD_SECS", COLIKE_REBUILD_SECS);
        tokio::spawn(async move {
            run_now_then_tick(Duration::from_secs(secs), shutdown_clone, |_| {
                let svc = service_inner.clone();
                async move {
                    match colike::rebuild(&svc.pg).await {
                        Ok(n) => info!(edges = n, "colike: rebuilt"),
                        Err(e) => warn!(error = %e, "colike rebuild failed"),
                    }
                }
            })
            .await;
        });
    }

    // Докачка каталога сеток: pending-треки артистов, которых волна активных
    // юзеров реально хочет отдавать, уходят в голову очереди скачки.
    {
        let service_inner = service.clone();
        let shutdown_clone = shutdown.clone();
        let secs = env_secs("RECS_WAVE_BUMP_SECS", WAVE_BUMP_SECS);
        tokio::spawn(async move {
            run_now_then_tick(Duration::from_secs(secs), shutdown_clone, |_| {
                let svc = service_inner.clone();
                async move {
                    match colike::bump_graph_storage_priority(&svc.pg).await {
                        Ok(0) => {}
                        Ok(n) => info!(n, "wave bump: storage priorities raised"),
                        Err(e) => warn!(error = %e, "wave bump failed"),
                    }
                }
            })
            .await;
        });
    }
}

fn env_secs(var: &str, default: u64) -> u64 {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n >= 60)
        .unwrap_or(default)
}

async fn run_now_then_tick<F, Fut>(period: Duration, shutdown: CancellationToken, mut tick: F)
where
    F: FnMut(()) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    tick(()).await;
    tick_with_shutdown(period, shutdown, tick).await;
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
