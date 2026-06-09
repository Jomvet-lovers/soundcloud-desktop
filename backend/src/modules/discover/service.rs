use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::cache::cache_service::CacheScope;
use crate::cache::CacheService;
use crate::error::AppResult;
use crate::modules::subscriptions::SubscriptionsService;

const REFRESH_TICK: Duration = Duration::from_secs(60 * 30);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const FRESH_WINDOW_DAYS: i32 = 14;
const TAG_PRECOMPUTE_LIMIT: i64 = 32;
const CACHE_TTL_FALLBACK_SECS: u64 = 3 * 60 * 60;
// Один наш вовлечённый full_play ≈ INTERNAL_PLAY_WEIGHT пассивных SC-плеев.
// Поднимает локальных фаворитов над глобальными по SC, не обгоняя реальные хиты.
const INTERNAL_PLAY_WEIGHT: i64 = 10_000;

pub const REDIS_KEY_SUMMARY: &str = "discover:summary:v1";
pub const REDIS_KEY_TAGS: &str = "discover:tags:v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSummary {
    pub artists_count: i64,
    pub albums_count: i64,
    pub fresh_count: i64,
    pub fresh_window_days: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTag {
    pub id: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTagList {
    pub items: Vec<CachedTag>,
}

pub struct DiscoverService {
    pg: PgPool,
    cache: Arc<CacheService>,
    subscriptions: Arc<SubscriptionsService>,
    // Process-local single-flight for `refresh_aggregates`: a manual trigger must
    // not stack bulk catalog UPDATEs on top of the periodic tick, or each other.
    refresh_lock: tokio::sync::Mutex<()>,
}

impl DiscoverService {
    pub fn new(
        pg: PgPool,
        cache: Arc<CacheService>,
        subscriptions: Arc<SubscriptionsService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            cache,
            subscriptions,
            refresh_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// Run `refresh_aggregates` under the single-flight guard. Returns `false`
    /// (no-op) if a refresh is already in flight on this instance.
    pub async fn try_refresh_aggregates(&self) -> AppResult<bool> {
        let Ok(_guard) = self.refresh_lock.try_lock() else {
            return Ok(false);
        };
        self.refresh_aggregates().await?;
        Ok(true)
    }

    pub fn spawn_refresh_loop(self: Arc<Self>, shutdown: CancellationToken) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(REFRESH_TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tick.tick().await;
            if let Err(e) = self.try_refresh_aggregates().await {
                warn!(error = %e, "discover bootstrap refresh failed");
            }
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = tick.tick() => {
                        match tokio::time::timeout(REFRESH_TIMEOUT, self.try_refresh_aggregates()).await {
                            Ok(Ok(_)) => {}
                            Ok(Err(e)) => warn!(error = %e, "discover refresh failed"),
                            Err(_) => warn!("discover refresh timed out"),
                        }
                    }
                }
            }
        });
    }

    pub async fn refresh_aggregates(&self) -> AppResult<()> {
        let started = std::time::Instant::now();
        self.refresh_artist_counts().await?;
        self.refresh_artist_plays().await?;
        self.refresh_artist_popularity().await?;
        self.refresh_artist_tags().await?;
        self.refresh_artist_star().await?;
        self.refresh_album_meta().await?;
        self.refresh_album_popularity().await?;
        self.refresh_album_star().await?;
        if let Err(e) = self.warm_redis_caches().await {
            warn!(error = %e, "discover redis warm failed");
        }
        info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "discover aggregates refreshed",
        );
        Ok(())
    }

    async fn refresh_artist_counts(&self) -> AppResult<()> {
        // track_count_primary считаем только по indexed tracks. Wanted-only
        // артисты (без реальных треков) не должны показываться в discover —
        // у юзера на их странице "вообще всё пусто".
        sqlx::query_file!("queries/discover/service/refresh_artist_counts.sql")
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn refresh_artist_plays(&self) -> AppResult<()> {
        sqlx::query_file!("queries/discover/service/refresh_artist_plays.sql")
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn refresh_artist_popularity(&self) -> AppResult<()> {
        // Гибрид: SC play_count по primary-трекам (база) + наши full_play с
        // весом INTERNAL_PLAY_WEIGHT. LN-нормализация как у album popularity.
        sqlx::query_file!(
            "queries/discover/service/refresh_artist_popularity.sql",
            INTERNAL_PLAY_WEIGHT
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn refresh_artist_tags(&self) -> AppResult<()> {
        sqlx::query_file!("queries/discover/service/refresh_artist_tags.sql")
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn refresh_artist_star(&self) -> AppResult<()> {
        let always_premium = self.subscriptions.always_premium();
        let now = chrono::Utc::now().timestamp();
        if always_premium {
            sqlx::query_file!("queries/discover/service/refresh_artist_star_premium.sql")
                .execute(&self.pg)
                .await?;
        } else {
            sqlx::query_file!(
                "queries/discover/service/refresh_artist_star_active.sql",
                now
            )
            .execute(&self.pg)
            .await?;
        }
        Ok(())
    }

    async fn refresh_album_meta(&self) -> AppResult<()> {
        sqlx::query_file!("queries/discover/service/refresh_album_meta.sql")
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn refresh_album_popularity(&self) -> AppResult<()> {
        sqlx::query_file!("queries/discover/service/refresh_album_popularity.sql")
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn refresh_album_star(&self) -> AppResult<()> {
        sqlx::query_file!("queries/discover/service/refresh_album_star.sql")
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    pub async fn compute_summary(&self) -> AppResult<CachedSummary> {
        // artists_count/albums_count/fresh_count считаем по тому же гейту, что и
        // каталог — иначе бейдж таба завышает на мусоре (артисты без треков,
        // альбомы без плеев), которого в самом каталоге нет.
        let row = sqlx::query_file!(
            "queries/discover/service/compute_summary.sql",
            FRESH_WINDOW_DAYS
        )
        .fetch_one(&self.pg)
        .await?;

        Ok(CachedSummary {
            artists_count: row.artists_count.max(0),
            albums_count: row.albums_count.max(0),
            fresh_count: row.fresh_count,
            fresh_window_days: FRESH_WINDOW_DAYS,
        })
    }

    pub async fn compute_tag_list(&self) -> AppResult<CachedTagList> {
        let rows = sqlx::query_file!(
            "queries/discover/service/compute_tag_list.sql",
            TAG_PRECOMPUTE_LIMIT
        )
        .fetch_all(&self.pg)
        .await?;

        Ok(CachedTagList {
            items: rows
                .into_iter()
                .map(|r| CachedTag {
                    id: r.tag,
                    count: r.n,
                })
                .collect(),
        })
    }

    async fn warm_redis_caches(&self) -> AppResult<()> {
        let summary = self.compute_summary().await?;
        let tags = self.compute_tag_list().await?;
        let payload_s = serde_json::to_string(&summary).unwrap_or_default();
        let payload_t = serde_json::to_string(&tags).unwrap_or_default();
        self.cache
            .set_raw(
                REDIS_KEY_SUMMARY,
                &payload_s,
                CACHE_TTL_FALLBACK_SECS,
                None,
                CacheScope::Shared,
                None,
            )
            .await?;
        self.cache
            .set_raw(
                REDIS_KEY_TAGS,
                &payload_t,
                CACHE_TTL_FALLBACK_SECS,
                None,
                CacheScope::Shared,
                None,
            )
            .await?;
        Ok(())
    }
}
