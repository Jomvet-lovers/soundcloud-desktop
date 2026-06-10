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
        let id = track.id;
        let ctx = TrackContext::from_row(&track);

        let mut result = if let Some(fast) = self.try_sc_verified(&ctx).await? {
            // Verified-клейм даёт только владельца аккаунта — co-авторов из
            // разметки ("мокери, psychosis - …") и меты ("takizava & dekma")
            // добираем тем же путём, что и для внешних источников.
            crate::modules::enrich::resolver::enrich_with_local_signals(fast, &ctx)
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
        let row = sqlx::query_file!(
            "queries/enrich/service/sc_verified_artist.sql",
            uploader_sc_id
        )
        .fetch_optional(&self.pg)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let name = row.name;
        let mb_id = row.mb_artist_id;
        let genius_id = row.genius_artist_id;

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
