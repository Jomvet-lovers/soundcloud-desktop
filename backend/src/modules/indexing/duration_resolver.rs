use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::error::AppResult;
use crate::modules::tracks::TrackRepository;
use crate::sc::ScReadService;

const TICK: Duration = Duration::from_secs(120);
const BATCH: i64 = 50;
const REQ_GAP: Duration = Duration::from_millis(150);

pub struct DurationResolver {
    tracks: TrackRepository,
    resolve: Arc<ScReadService>,
    max_track_duration_ms: i32,
}

impl DurationResolver {
    pub fn new(pg: PgPool, resolve: Arc<ScReadService>, max_track_duration_ms: i32) -> Arc<Self> {
        let tracks = TrackRepository::new(pg);
        Arc::new(Self {
            tracks,
            resolve,
            max_track_duration_ms,
        })
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = me.tick().await {
                            warn!(error = %e, "duration_resolver tick failed");
                        }
                    }
                }
            }
        });
    }

    async fn tick(&self) -> AppResult<()> {
        let ids = self.tracks.pick_duration_resolve(BATCH).await?;
        if ids.is_empty() {
            return Ok(());
        }
        for sc_track_id in ids {
            tokio::time::sleep(REQ_GAP).await;
            if let Err(e) = self.resolve_one(&sc_track_id).await {
                debug!(track = %sc_track_id, error = %e, "duration_resolve: skip");
                let _ = self.tracks.clear_duration_resolve(&sc_track_id).await;
            }
        }
        Ok(())
    }

    async fn resolve_one(&self, sc_track_id: &str) -> AppResult<()> {
        let v = self.resolve.fetch_track_v2(sc_track_id).await?;
        let full = v
            .get("full_duration")
            .and_then(|x| x.as_i64())
            .or_else(|| v.get("duration").and_then(|x| x.as_i64()))
            .unwrap_or(0);
        if full > 0 && full != 30_000 {
            self.tracks
                .apply_resolved_duration(sc_track_id, full as i32)
                .await?;
            if self.max_track_duration_ms > 0 && full > self.max_track_duration_ms as i64 {
                self.tracks.mark_too_long(sc_track_id).await?;
            }
        } else {
            // SC по-прежнему отдаёт sentinel — снимаем флаг, чтобы не зацикливать
            // запросы; реальный duration появится при следующем cold-refresh'е
            // когда трек разморозится.
            self.tracks.clear_duration_resolve(sc_track_id).await?;
        }
        Ok(())
    }
}
