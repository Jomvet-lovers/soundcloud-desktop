use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::config::EnrichCfg;
use crate::error::{AppError, AppResult};
use crate::modules::enrich::ai::AiResolverClient;
use crate::modules::enrich::coplay;
use crate::modules::enrich::mb::MbClient;
use crate::modules::enrich::persist;
use crate::modules::enrich::resolver::{resolve, ResolveSource, ResolverDeps, TrackContext};
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
            pg,
            deps: ResolverDeps { mb, genius, ai },
            cfg,
        })
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
        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
               COUNT(*) FILTER (WHERE enrich_state = 'pending')::int8,
               COUNT(*) FILTER (WHERE enrich_state = 'done')::int8,
               COUNT(*) FILTER (WHERE enrich_state = 'failed')::int8,
               COUNT(*) FILTER (WHERE enrich_state = 'dead')::int8,
               COUNT(*) FILTER (WHERE enrich_locked_at IS NOT NULL)::int8
             FROM tracks",
        )
        .fetch_one(&self.pg)
        .await?;
        let albums: (i64,) = sqlx::query_as("SELECT COUNT(*)::int8 FROM albums")
            .fetch_one(&self.pg)
            .await?;
        let c: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
               COUNT(*)::int8,
               COUNT(*) FILTER (WHERE genius_artist_id IS NOT NULL)::int8,
               COUNT(*) FILTER (WHERE genius_crawled_at IS NOT NULL)::int8,
               COUNT(*) FILTER (WHERE mb_artist_id IS NOT NULL)::int8,
               COUNT(*) FILTER (WHERE mb_crawled_at IS NOT NULL)::int8,
               COUNT(*) FILTER (WHERE NOT crawl_dead AND (genius_artist_id IS NOT NULL OR mb_artist_id IS NOT NULL)
                                    AND (genius_next_run_at <= now() OR mb_next_run_at <= now()))::int8,
               COUNT(*) FILTER (WHERE crawl_dead)::int8
             FROM artists WHERE merged_into IS NULL",
        )
            .fetch_one(&self.pg)
            .await?;
        let w: (i64, i64) = sqlx::query_as(
            "SELECT
               COUNT(*) FILTER (WHERE status = 'wanted' AND track_id IS NULL)::int8,
               COUNT(*) FILTER (WHERE status = 'unresolvable')::int8
             FROM wanted_tracks",
        )
            .fetch_one(&self.pg)
            .await?;
        Ok(EnrichStats {
            pending: row.0,
            done: row.1,
            failed: row.2,
            dead: row.3,
            in_flight: row.4,
            artists: c.0,
            albums: albums.0,
            crawl: CrawlStats {
                artists_total: c.0,
                genius_total: c.1,
                genius_crawled: c.2,
                mb_total: c.3,
                mb_crawled: c.4,
                due_now: c.5,
                dead: c.6,
            },
            wanted: WantedStats {
                wanted: w.0,
                unresolvable: w.1,
            },
        })
    }

    /// Resolve + persist one track. Called by `EnrichSource::run` (claimed +
    /// leased by the pool) and by nothing else. Holds no pooled connection
    /// across the resolver's external I/O.
    pub async fn process_track(&self, sc_track_id: &str) -> AppResult<()> {
        let track_row: Option<crate::modules::tracks::TrackRow> =
            sqlx::query_as("SELECT * FROM tracks WHERE sc_track_id = $1")
                .bind(sc_track_id)
                .fetch_optional(&self.pg)
                .await?;
        let Some(track) = track_row else {
            return Ok(());
        };
        let id = track.id;
        let ctx = TrackContext::from_row(&track);

        let mut result = if let Some(fast) = self.try_sc_verified(&ctx).await? {
            fast
        } else {
            resolve(&ctx, &self.deps).await?
        };

        if result.primary.is_empty() {
            return Err(AppError::internal(format!(
                "no primary artist resolved for {sc_track_id}"
            )));
        }

        if matches!(result.source, ResolveSource::Heuristic) {
            if let Some(ai) = self.deps.ai.as_ref() {
                let primary_name = result.primary.first().map(|a| a.name.clone());
                if let Some(name) = primary_name {
                    let title_q = if ctx.title.contains(" - ") {
                        ctx.title
                            .split(" - ")
                            .last()
                            .unwrap_or(&ctx.title)
                            .trim()
                            .to_string()
                    } else {
                        ctx.title.clone()
                    };
                    match ai.verify_existence(&name, &title_q).await {
                        Ok(Some(true)) => {
                            result.confidence = result.confidence.max(0.4);
                        }
                        Ok(Some(false)) => {
                            result.confidence = result.confidence.min(0.05);
                        }
                        _ => {}
                    }
                }
            }
        }

        let outcome = persist::apply(
            &self.pg,
            id,
            &result,
            ctx.uploader_sc_user_id.as_deref(),
            ctx.uploader_username.as_deref(),
        )
        .await?;
        if outcome.coplay_dirty {
            if let Err(e) = coplay::recompute_for_track(&self.pg, id).await {
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

    async fn try_sc_verified(
        &self,
        ctx: &TrackContext,
    ) -> AppResult<Option<crate::modules::enrich::resolver::ResolveResult>> {
        let Some(uploader_sc_id) = ctx.uploader_sc_user_id.as_deref() else {
            return Ok(None);
        };
        if uploader_sc_id.is_empty() {
            return Ok(None);
        }
        let row: Option<(Uuid, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT a.id, a.name, a.mb_artist_id, a.genius_artist_id
             FROM artist_sc_accounts asa
             JOIN artists a ON a.id = asa.artist_id
             WHERE asa.sc_user_id = $1 AND a.merged_into IS NULL
             LIMIT 1",
        )
            .bind(uploader_sc_id)
        .fetch_optional(&self.pg)
        .await?;
        let Some((_, name, mb_id, genius_id)) = row else {
            return Ok(None);
        };

        let parsed = crate::modules::enrich::normalize::parse_sc_title(
            &ctx.title,
            ctx.uploader_username.as_deref(),
        );
        let mapped_norm = crate::modules::enrich::normalize::normalize_name(&name);
        let title_claims_other = parsed
            .primary_artists
            .first()
            .map(|p| crate::modules::enrich::normalize::normalize_name(p) != mapped_norm)
            .unwrap_or(false);
        if title_claims_other {
            debug!(
                uploader_sc_id,
                mapped = %name,
                parsed = ?parsed.primary_artists,
                "sc_verified skipped: title claims different artist"
            );
            return Ok(None);
        }

        use crate::modules::enrich::resolver::{ArtistCandidate, ResolveResult, ResolveSource};
        Ok(Some(ResolveResult {
            source: ResolveSource::ScVerified,
            confidence: 1.0,
            primary: vec![ArtistCandidate {
                name,
                mb_id,
                genius_id,
                sc_user_id: Some(uploader_sc_id.to_string()),
            }],
            featured: Vec::new(),
            producers: Vec::new(),
            remixers: Vec::new(),
            album: None,
            isrc: ctx.isrc.clone(),
            release_date: None,
            release_year: None,
            is_cover: false,
        }))
    }
}
