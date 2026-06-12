use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::config::EnrichCfg;
use crate::error::{AppError, AppResult};
use crate::modules::enrich::ai::AiResolverClient;
use crate::modules::enrich::coplay;
use crate::modules::enrich::mb::MbClient;
use crate::modules::enrich::persist;
use crate::modules::enrich::resolver::{self, ResolveSource, ResolverDeps};
use crate::modules::enrich::source::EnrichSource;
use crate::modules::lyrics::genius::GeniusService;
use crate::modules::work::{self, Kicker, SchedulerPolicy};

const TICK: Duration = Duration::from_secs(2);
const LEASE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
pub struct EnrichStats {
    pub pending: i64,
    pub done: i64,
    pub failed: i64,
    pub dead: i64,
    pub in_flight: i64,
    pub artists: i64,
    pub albums: i64,
    pub crawl: CrawlStats,
    pub wanted: WantedStats,
}

/// Catalog crawl coverage — the "% of artists ever walked" invariant.
#[derive(Debug, Clone, Serialize)]
pub struct CrawlStats {
    pub artists_total: i64,
    pub genius_total: i64,
    pub genius_crawled: i64,
    pub mb_total: i64,
    pub mb_crawled: i64,
    pub due_now: i64,
    pub dead: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WantedStats {
    pub wanted: i64,
    pub unresolvable: i64,
}

pub struct EnrichService {
    pg: PgPool,
    deps: ResolverDeps,
    cfg: EnrichCfg,
}

impl EnrichService {
    pub fn new(
        pg: PgPool,
        mb: Arc<MbClient>,
        genius: Arc<GeniusService>,
        ai: Option<Arc<AiResolverClient>>,
        cfg: EnrichCfg,
    ) -> Arc<Self> {
        Arc::new(Self {
            deps: ResolverDeps {
                mb,
                genius,
                ai,
                pg: pg.clone(),
            },
            pg,
            cfg,
        })
    }

    /// MB-клиент для maintenance-проходов (имена сущностей и т.п.).
    pub fn mb(&self) -> Arc<MbClient> {
        self.deps.mb.clone()
    }

    /// Build the enrich worker pool over `tracks` and return the kick sender for
    /// the ingest fast path. No NATS — the durable work-list is Postgres.
    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) -> Option<Kicker> {
        if !self.cfg.enabled {
            info!("enrich disabled by config");
            return None;
        }
        let concurrency = self.cfg.consumer_concurrency.max(1);
        let source = Arc::new(EnrichSource::new(
            self.pg.clone(),
            self.clone(),
            self.cfg.max_attempts as i16,
        ));
        let policy = SchedulerPolicy {
            name: "enrich",
            concurrency,
            batch: (concurrency * 4) as i64,
            tick: TICK,
            lease_timeout: LEASE_TIMEOUT,
        };
        Some(work::spawn(source, policy, shutdown))
    }

    pub async fn stats(&self) -> AppResult<EnrichStats> {
        let row = sqlx::query_file!("queries/enrich/service/stats_tracks.sql")
            .fetch_one(&self.pg)
            .await?;
        let albums = sqlx::query_file_scalar!("queries/enrich/service/stats_albums.sql")
            .fetch_one(&self.pg)
            .await?;
        let c = sqlx::query_file!("queries/enrich/service/stats_crawl.sql")
            .fetch_one(&self.pg)
            .await?;
        let w = sqlx::query_file!("queries/enrich/service/stats_wanted.sql")
            .fetch_one(&self.pg)
            .await?;
        Ok(EnrichStats {
            pending: row.pending,
            done: row.done,
            failed: row.failed,
            dead: row.dead,
            in_flight: row.in_flight,
            artists: c.artists_total,
            albums,
            crawl: CrawlStats {
                artists_total: c.artists_total,
                genius_total: c.genius_total,
                genius_crawled: c.genius_crawled,
                mb_total: c.mb_total,
                mb_crawled: c.mb_crawled,
                due_now: c.due_now,
                dead: c.dead,
            },
            wanted: WantedStats {
                wanted: w.wanted,
                unresolvable: w.unresolvable,
            },
        })
    }

    /// Resolve + persist one track. Called by `EnrichSource::run` (claimed +
    /// leased by the pool) and by nothing else. Holds no pooled connection
    /// across the resolver's external I/O.
    pub async fn process_track(&self, sc_track_id: &str) -> AppResult<()> {
        let track_row = sqlx::query_file_as!(
            crate::modules::tracks::TrackRow,
            "queries/enrich/service/track_by_sc_id.sql",
            sc_track_id
        )
        .fetch_optional(&self.pg)
        .await?;
        let Some(track) = track_row else {
            return Ok(());
        };

        let result = resolver::resolve_track(&track, &self.deps).await?;
        if result.primary.is_empty() {
            return Err(AppError::internal(format!(
                "no primary artist resolved for {sc_track_id}"
            )));
        }
        // Каскад деградировал из-за транзиентного отказа источника, а прошлый
        // результат сильнее нового — не даунгрейдим (persist не зовём, старые
        // связи целы), отдаём трек в ретрай по бэкоффу.
        if result.degraded {
            let prev_source = track.enrich_source.as_deref().unwrap_or("");
            if ResolveSource::priority_of(prev_source) > result.source.priority() {
                return Err(AppError::internal(format!(
                    "transient source failure; keeping prior '{prev_source}' enrichment"
                )));
            }
        }

        let outcome = persist::apply(
            &self.pg,
            track.id,
            &result,
            track.uploader_sc_user_id.as_deref(),
            track.uploader_username.as_deref(),
        )
        .await?;
        if outcome.coplay_dirty {
            if let Err(e) = coplay::recompute_for_track(&self.pg, track.id).await {
                warn!(track = %sc_track_id, error = %e, "coplay recompute failed");
            }
        }
        debug!(
            track = %sc_track_id,
            primary = ?outcome.primary_artist_id,
            album = ?outcome.album_id,
            source = result.source.as_str(),
            confidence = result.confidence,
            "enriched"
        );
        Ok(())
    }
}
