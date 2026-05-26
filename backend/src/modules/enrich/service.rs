use std::sync::Arc;
use std::time::Duration;

use mini_moka::sync::Cache;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::bus::nats::NatsService;
use crate::bus::subjects::{self, streams};
use crate::common::sc_ids::normalize_sc_track_id;
use crate::config::EnrichCfg;
use crate::error::{AppError, AppResult};
use crate::modules::enrich::ai::AiResolverClient;
use crate::modules::enrich::artist_crawl::ArtistCrawlService;
use crate::modules::enrich::coplay;
use crate::modules::enrich::mb::MbClient;
use crate::modules::enrich::persist;
use crate::modules::enrich::resolver::{resolve, ResolveSource, ResolverDeps, TrackContext};
use crate::modules::enrich::wanted_resolver::WantedResolverService;
use crate::modules::lyrics::genius::GeniusService;

const INFLIGHT_CAPACITY: u64 = 8192;
const INFLIGHT_TTL: Duration = Duration::from_secs(120);
const FRESH_AFTER_DONE: chrono::Duration = chrono::Duration::hours(24);
const ALBUM_INGEST_CAPACITY: u64 = 4096;
const ALBUM_INGEST_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichJob {
    pub sc_track_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnrichStats {
    pub pending: i64,
    pub done: i64,
    pub failed: i64,
    pub artists: i64,
    pub albums: i64,
}

pub struct EnrichService {
    pg: PgPool,
    nats: Arc<NatsService>,
    deps: ResolverDeps,
    cfg: EnrichCfg,
    inflight: Cache<String, ()>,
    album_ingest_inflight: Cache<Uuid, ()>,
    crawl: OnceCell<Arc<ArtistCrawlService>>,
    wanted_resolver: OnceCell<Arc<WantedResolverService>>,
}

impl EnrichService {
    pub fn new(
        pg: PgPool,
        nats: Arc<NatsService>,
        mb: Arc<MbClient>,
        genius: Arc<GeniusService>,
        ai: Option<Arc<AiResolverClient>>,
        cfg: EnrichCfg,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            nats,
            deps: ResolverDeps { mb, genius, ai },
            cfg,
            inflight: Cache::builder()
                .max_capacity(INFLIGHT_CAPACITY)
                .time_to_idle(INFLIGHT_TTL)
                .build(),
            album_ingest_inflight: Cache::builder()
                .max_capacity(ALBUM_INGEST_CAPACITY)
                .time_to_live(ALBUM_INGEST_TTL)
                .build(),
            crawl: OnceCell::new(),
            wanted_resolver: OnceCell::new(),
        })
    }

    pub fn install_followup(
        &self,
        crawl: Arc<ArtistCrawlService>,
        wanted_resolver: Arc<WantedResolverService>,
    ) {
        let _ = self.crawl.set(crawl);
        let _ = self.wanted_resolver.set(wanted_resolver);
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        if !self.cfg.enabled {
            info!("enrich disabled by config");
            return;
        }
        self.subscribe_enrich();
        self.spawn_backfill_loop(shutdown);
    }

    pub async fn stats(&self) -> AppResult<EnrichStats> {
        let row: (i64, i64, i64) = sqlx::query_as(
            "SELECT
               COUNT(*) FILTER (WHERE enrich_state = 'pending')::int8,
               COUNT(*) FILTER (WHERE enrich_state = 'done')::int8,
               COUNT(*) FILTER (WHERE enrich_state = 'failed')::int8
             FROM tracks",
        )
        .fetch_one(&self.pg)
        .await?;
        let artists: (i64,) = sqlx::query_as("SELECT COUNT(*)::int8 FROM artists")
            .fetch_one(&self.pg)
            .await?;
        let albums: (i64,) = sqlx::query_as("SELECT COUNT(*)::int8 FROM albums")
            .fetch_one(&self.pg)
            .await?;
        Ok(EnrichStats {
            pending: row.0,
            done: row.1,
            failed: row.2,
            artists: artists.0,
            albums: albums.0,
        })
    }

    fn subscribe_enrich(self: &Arc<Self>) {
        let svc = self.clone();
        self.nats.consume(
            streams::ENRICH.name,
            "backend-enrich-track",
            Some(subjects::ENRICH_TRACK),
            move |data| {
                let svc = svc.clone();
                async move { svc.handle_message(data).await }
            },
        );
    }

    async fn handle_message(&self, data: Value) -> AppResult<()> {
        let sc_track_id = data
            .get("sc_track_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();
        let Some(sc_track_id) = normalize_sc_track_id(&sc_track_id) else {
            return Ok(());
        };

        if self.inflight.contains_key(&sc_track_id) {
            return Ok(());
        }
        self.inflight.insert(sc_track_id.clone(), ());

        let result = self.process(&sc_track_id).await;
        self.inflight.invalidate(&sc_track_id);

        if let Err(e) = result {
            warn!(track = %sc_track_id, error = %e, "enrich failed");
            if let Ok(Some(id)) = self.fetch_indexed_id(&sc_track_id).await {
                let _ = persist::mark_failed(&self.pg, id, &e.to_string()).await;
            }
        }
        Ok(())
    }

    async fn try_sc_verified(
        &self,
        ctx: &crate::modules::enrich::resolver::TrackContext,
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
            tracing::debug!(
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

    async fn fetch_indexed_id(&self, sc_track_id: &str) -> AppResult<Option<Uuid>> {
        let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM tracks WHERE sc_track_id = $1")
            .bind(sc_track_id)
            .fetch_optional(&self.pg)
            .await?;
        Ok(row.map(|(id,)| id))
    }

    async fn process(&self, sc_track_id: &str) -> AppResult<()> {
        let track_row: Option<crate::modules::tracks::TrackRow> =
            sqlx::query_as("SELECT * FROM tracks WHERE sc_track_id = $1")
                .bind(sc_track_id)
                .fetch_optional(&self.pg)
                .await?;
        let Some(track) = track_row else {
            return Ok(());
        };
        let id = track.id;
        let state = track.enrich_state.clone();
        let enriched_at = track.enriched_at;

        let now = chrono::Utc::now();
        if state == "done" {
            if let Some(t) = enriched_at {
                if now - t < FRESH_AFTER_DONE {
                    return Ok(());
                }
            }
        }

        // Cross-process lock: гарантирует, что один и тот же sc_track_id не
        // обрабатывается двумя инстансами / двумя redelivery NATS параллельно.
        // Lock привязан к сессии postgres'а — освободится сам при возврате
        // connection в pool, либо мы явно снимем в конце.
        let mut lock_conn = self.pg.acquire().await?;
        let locked: (bool,) =
            sqlx::query_as("SELECT pg_try_advisory_lock(hashtextextended($1::text, 0))")
                .bind(sc_track_id)
                .fetch_one(&mut *lock_conn)
                .await?;
        if !locked.0 {
            tracing::debug!(track = %sc_track_id, "enrich: skipped (locked elsewhere)");
            return Ok(());
        }
        let _guard = AdvisoryLockGuard::new(lock_conn, sc_track_id.to_string());

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
        if let Some(primary_artist_id) = outcome.primary_artist_id {
            self.maybe_kick_followup(primary_artist_id).await;
            if let Some(album_id) = outcome.album_id {
                if let Some(genius_album_id) = result
                    .album
                    .as_ref()
                    .and_then(|a| a.genius_id.as_deref())
                    .and_then(|s| s.parse::<i64>().ok())
                {
                    self.maybe_ingest_album_tracks(primary_artist_id, album_id, genius_album_id);
                }
            }
        }
        Ok(())
    }

    fn maybe_ingest_album_tracks(&self, artist_id: Uuid, album_id: Uuid, genius_album_id: i64) {
        if self.album_ingest_inflight.contains_key(&album_id) {
            return;
        }
        self.album_ingest_inflight.insert(album_id, ());
        let Some(crawl) = self.crawl.get().cloned() else {
            return;
        };
        let resolver = self.wanted_resolver.get().cloned();
        tokio::spawn(async move {
            if let Err(e) = crawl
                .ingest_genius_album_tracks(artist_id, album_id, genius_album_id)
                .await
            {
                warn!(%album_id, error = %e, "album-tracks ingest failed");
                return;
            }
            if let Some(resolver) = resolver {
                if let Err(e) = resolver.run_for_artist(artist_id, 100).await {
                    debug!(%artist_id, error = %e, "post-album wanted-resolver failed");
                }
            }
        });
    }

    async fn maybe_kick_followup(&self, artist_id: Uuid) {
        type FollowupRow = (
            Option<String>,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
        );
        let row: Option<FollowupRow> = match sqlx::query_as(
            "SELECT mb_artist_id, genius_artist_id, last_crawled_at
             FROM artists WHERE id = $1 AND merged_into IS NULL",
        )
        .bind(artist_id)
        .fetch_optional(&self.pg)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, %artist_id, "followup: lookup artist failed");
                return;
            }
        };
        let Some((mb_id, genius_id, last_crawled_at)) = row else {
            return;
        };
        if mb_id.is_none() && genius_id.is_none() {
            return;
        }
        if last_crawled_at.is_some() {
            return;
        }
        let Some(crawl) = self.crawl.get().cloned() else {
            return;
        };
        let resolver = self.wanted_resolver.get().cloned();
        tokio::spawn(async move {
            if let Err(e) = crawl.run_for_artist(artist_id).await {
                warn!(%artist_id, error = %e, "followup: crawl failed");
                return;
            }
            if let Some(resolver) = resolver {
                if let Err(e) = resolver.run_for_artist(artist_id, 500).await {
                    warn!(%artist_id, error = %e, "followup: wanted-resolver failed");
                }
            }
        });
    }

    fn spawn_backfill_loop(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        tokio::spawn(async move {
            let interval = Duration::from_secs(svc.cfg.backfill_interval_sec.max(10));
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = svc.run_backfill_tick().await {
                            warn!(error = %e, "enrich backfill tick failed");
                        }
                    }
                }
            }
        });
    }

    async fn run_backfill_tick(&self) -> AppResult<()> {
        let max_attempts = self.cfg.max_attempts as i16;
        let batch = self.cfg.backfill_batch.max(1);
        let rows: Vec<(String,)> = sqlx::query_as(
            "WITH picked AS (
                 SELECT id
                 FROM tracks
                 WHERE enrich_state IN ('pending', 'failed')
                   AND enrich_attempts < $1
                   AND (enriched_at IS NULL
                        OR enriched_at < now() - (interval '5 minutes' * power(2, enrich_attempts)))
                 ORDER BY enriched_at NULLS FIRST
                 LIMIT $2
                 FOR UPDATE SKIP LOCKED
             )
             UPDATE tracks t
             SET enriched_at = now()
             FROM picked
             WHERE t.id = picked.id
             RETURNING t.sc_track_id",
        )
        .bind(max_attempts)
        .bind(batch)
        .fetch_all(&self.pg)
        .await?;

        if rows.is_empty() {
            return Ok(());
        }
        info!(count = rows.len(), "enrich backfill: republishing");
        for (sc_track_id,) in rows {
            let job = EnrichJob {
                sc_track_id: sc_track_id.clone(),
            };
            if let Err(e) = self.nats.publish(subjects::ENRICH_TRACK, &job).await {
                warn!(track = %sc_track_id, error = %e, "enrich backfill publish failed");
            }
        }
        Ok(())
    }
}

/// RAII guard для pg_advisory_unlock. При drop'е спавнит fire-and-forget таск,
/// который снимает lock — асинхронно, потому что Drop sync. Если процесс
/// упадёт до выполнения — postgres сам отпустит lock при reset'е connection.
struct AdvisoryLockGuard {
    conn: Option<sqlx::pool::PoolConnection<sqlx::Postgres>>,
    sc_track_id: String,
}

impl AdvisoryLockGuard {
    fn new(conn: sqlx::pool::PoolConnection<sqlx::Postgres>, sc_track_id: String) -> Self {
        Self {
            conn: Some(conn),
            sc_track_id,
        }
    }
}

impl Drop for AdvisoryLockGuard {
    fn drop(&mut self) {
        let Some(mut conn) = self.conn.take() else {
            return;
        };
        let id = std::mem::take(&mut self.sc_track_id);
        tokio::spawn(async move {
            let _ = sqlx::query("SELECT pg_advisory_unlock(hashtextextended($1::text, 0))")
                .bind(&id)
                .execute(&mut *conn)
                .await;
        });
    }
}

pub async fn publish_enrich(nats: &NatsService, sc_track_id: &str) -> AppResult<()> {
    let Some(id) = normalize_sc_track_id(sc_track_id) else {
        return Ok(());
    };
    let job = EnrichJob { sc_track_id: id };
    nats.publish(subjects::ENRICH_TRACK, &job).await
}
