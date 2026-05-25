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
        })
    }

    pub fn spawn_refresh_loop(self: Arc<Self>, shutdown: CancellationToken) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(REFRESH_TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tick.tick().await;
            if let Err(e) = self.refresh_aggregates().await {
                warn!(error = %e, "discover bootstrap refresh failed");
            }
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = tick.tick() => {
                        match tokio::time::timeout(REFRESH_TIMEOUT, self.refresh_aggregates()).await {
                            Ok(Ok(())) => {}
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
        sqlx::query(
            r#"
            WITH primary_counts AS (
                SELECT artist_id, COUNT(*)::int AS n
                FROM track_artists WHERE role = 'primary'
                GROUP BY artist_id
            ),
            featured_counts AS (
                SELECT artist_id, COUNT(DISTINCT track_id)::int AS n
                FROM track_artists WHERE role IN ('featured', 'remixer')
                GROUP BY artist_id
            ),
            album_total AS (
                SELECT artist_id, COUNT(DISTINCT album_id)::int AS n
                FROM (
                    SELECT primary_artist_id AS artist_id, id AS album_id
                    FROM albums WHERE primary_artist_id IS NOT NULL
                    UNION
                    SELECT artist_id, album_id FROM album_artists
                ) x
                GROUP BY artist_id
            ),
            affected AS (
                SELECT artist_id FROM primary_counts
                UNION
                SELECT artist_id FROM featured_counts
                UNION
                SELECT artist_id FROM album_total
                UNION
                SELECT id AS artist_id FROM artists
                WHERE (track_count_primary  > 0
                    OR track_count_featured > 0
                    OR album_count_denorm   > 0)
                  AND merged_into IS NULL
            )
            UPDATE artists a SET
                track_count_primary  = COALESCE(pc.n, 0),
                track_count_featured = COALESCE(fc.n, 0),
                album_count_denorm   = COALESCE(at_.n, 0),
                aggregates_updated_at = NOW()
            FROM affected aff
            LEFT JOIN primary_counts  pc ON pc.artist_id = aff.artist_id
            LEFT JOIN featured_counts fc ON fc.artist_id = aff.artist_id
            LEFT JOIN album_total     at_ ON at_.artist_id = aff.artist_id
            WHERE a.id = aff.artist_id AND a.merged_into IS NULL
            "#,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn refresh_artist_plays(&self) -> AppResult<()> {
        sqlx::query(
            r#"
            WITH p30_listeners AS (
                SELECT it.primary_artist_id AS artist_id,
                       COUNT(DISTINCT ue.sc_user_id)::bigint AS listeners
                FROM user_events ue
                JOIN tracks it ON it.sc_track_id = ue.sc_track_id
                WHERE ue.event_type IN ('full_play', 'like', 'playlist_add')
                  AND ue.created_at > NOW() - INTERVAL '30 days'
                  AND it.primary_artist_id IS NOT NULL
                GROUP BY it.primary_artist_id
            ),
            p7_count AS (
                SELECT it.primary_artist_id AS artist_id, COUNT(*)::bigint AS plays
                FROM user_events ue
                JOIN tracks it ON it.sc_track_id = ue.sc_track_id
                WHERE ue.event_type = 'full_play'
                  AND ue.created_at > NOW() - INTERVAL '7 days'
                  AND it.primary_artist_id IS NOT NULL
                GROUP BY it.primary_artist_id
            ),
            p30_count AS (
                SELECT it.primary_artist_id AS artist_id, COUNT(*)::bigint AS plays
                FROM user_events ue
                JOIN tracks it ON it.sc_track_id = ue.sc_track_id
                WHERE ue.event_type = 'full_play'
                  AND ue.created_at > NOW() - INTERVAL '30 days'
                  AND it.primary_artist_id IS NOT NULL
                GROUP BY it.primary_artist_id
            ),
            affected AS (
                SELECT artist_id FROM p30_listeners
                UNION
                SELECT artist_id FROM p30_count
                UNION
                SELECT id AS artist_id FROM artists
                WHERE (monthly_listeners > 0 OR trending_score > 0)
                  AND merged_into IS NULL
            )
            UPDATE artists a SET
                monthly_listeners = COALESCE(p30l.listeners, 0),
                trending_score = LEAST(
                    1.0::real,
                    GREATEST(
                        0.0::real,
                        ( (COALESCE(p7.plays, 0)::real * 30.0)
                          / (7.0 * (COALESCE(p30.plays, 0)::real + 1.0))
                          - 0.5
                        ) / 3.0
                    )
                )
            FROM affected aff
            LEFT JOIN p30_listeners p30l ON p30l.artist_id = aff.artist_id
            LEFT JOIN p7_count       p7  ON p7.artist_id  = aff.artist_id
            LEFT JOIN p30_count      p30 ON p30.artist_id = aff.artist_id
            WHERE a.id = aff.artist_id AND a.merged_into IS NULL
            "#,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn refresh_artist_tags(&self) -> AppResult<()> {
        sqlx::query(
            r#"
            WITH per_artist AS (
                SELECT primary_artist_id AS artist_id,
                       LOWER(TRIM(genre)) AS g,
                       COUNT(*) AS cnt
                FROM tracks
                WHERE primary_artist_id IS NOT NULL
                  AND genre IS NOT NULL
                  AND TRIM(genre) <> ''
                GROUP BY primary_artist_id, LOWER(TRIM(genre))
            ),
            ranked AS (
                SELECT artist_id, g,
                       ROW_NUMBER() OVER (PARTITION BY artist_id ORDER BY cnt DESC, g) AS rk
                FROM per_artist
            ),
            top AS (
                SELECT artist_id,
                       ARRAY_AGG(g ORDER BY rk) FILTER (WHERE rk <= 3) AS tags
                FROM ranked
                GROUP BY artist_id
            ),
            affected AS (
                SELECT artist_id FROM top
                UNION
                SELECT id AS artist_id FROM artists
                WHERE array_length(tags, 1) IS NOT NULL AND merged_into IS NULL
            )
            UPDATE artists a SET tags = COALESCE(t.tags, '{}'::text[])
            FROM affected aff
            LEFT JOIN top t ON t.artist_id = aff.artist_id
            WHERE a.id = aff.artist_id AND a.merged_into IS NULL
            "#,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn refresh_artist_star(&self) -> AppResult<()> {
        let always_premium = self.subscriptions.always_premium();
        let now = chrono::Utc::now().timestamp();
        let sql = if always_premium {
            r#"
            WITH ranked AS (
                SELECT asa.artist_id,
                       ua.aura_id,
                       ua.custom_hex,
                       ROW_NUMBER() OVER (
                           PARTITION BY asa.artist_id
                           ORDER BY asa.verified DESC,
                                    CASE asa.role WHEN 'main' THEN 0 WHEN 'demo' THEN 1 ELSE 2 END,
                                    asa.sc_user_id
                       ) AS rk
                FROM artist_sc_accounts asa
                LEFT JOIN user_auras ua
                       ON ua.user_urn = 'soundcloud:users:' || asa.sc_user_id
                WHERE asa.role IN ('main', 'demo')
            ),
            star AS (
                SELECT artist_id, aura_id, custom_hex FROM ranked WHERE rk = 1
            ),
            affected AS (
                SELECT artist_id FROM star
                UNION
                SELECT id AS artist_id FROM artists
                WHERE is_star = TRUE AND merged_into IS NULL
            )
            UPDATE artists a SET
                is_star = (s.artist_id IS NOT NULL),
                star_aura_id = s.aura_id,
                star_custom_hex = s.custom_hex
            FROM affected aff
            LEFT JOIN star s ON s.artist_id = aff.artist_id
            WHERE a.id = aff.artist_id AND a.merged_into IS NULL
            "#
            .to_string()
        } else {
            r#"
            WITH ranked AS (
                SELECT asa.artist_id,
                       ua.aura_id,
                       ua.custom_hex,
                       ROW_NUMBER() OVER (
                           PARTITION BY asa.artist_id
                           ORDER BY asa.verified DESC,
                                    CASE asa.role WHEN 'main' THEN 0 WHEN 'demo' THEN 1 ELSE 2 END,
                                    asa.sc_user_id
                       ) AS rk
                FROM artist_sc_accounts asa
                JOIN subscriptions s
                       ON s.user_urn = 'soundcloud:users:' || asa.sc_user_id
                LEFT JOIN user_auras ua
                       ON ua.user_urn = 'soundcloud:users:' || asa.sc_user_id
                WHERE asa.role IN ('main', 'demo') AND s.exp_date > $1
            ),
            star AS (
                SELECT artist_id, aura_id, custom_hex FROM ranked WHERE rk = 1
            ),
            affected AS (
                SELECT artist_id FROM star
                UNION
                SELECT id AS artist_id FROM artists
                WHERE is_star = TRUE AND merged_into IS NULL
            )
            UPDATE artists a SET
                is_star = (s.artist_id IS NOT NULL),
                star_aura_id = s.aura_id,
                star_custom_hex = s.custom_hex
            FROM affected aff
            LEFT JOIN star s ON s.artist_id = aff.artist_id
            WHERE a.id = aff.artist_id AND a.merged_into IS NULL
            "#
            .to_string()
        };

        if always_premium {
            sqlx::query(&sql).execute(&self.pg).await?;
        } else {
            sqlx::query(&sql).bind(now).execute(&self.pg).await?;
        }
        Ok(())
    }

    async fn refresh_album_meta(&self) -> AppResult<()> {
        sqlx::query(
            r#"
            WITH album_meta AS (
                SELECT at.album_id,
                       COUNT(*)::int AS track_count,
                       COALESCE(SUM(it.duration_ms), 0)::bigint AS total_ms,
                       MIN(it.release_date) AS earliest_release
                FROM album_tracks at
                JOIN tracks it ON it.id = at.track_id
                GROUP BY at.album_id
            ),
            affected AS (
                SELECT album_id FROM album_meta
                UNION
                SELECT id AS album_id FROM albums
                WHERE track_count > 0 OR total_duration_ms > 0
            )
            UPDATE albums al SET
                track_count       = COALESCE(am.track_count, 0),
                total_duration_ms = COALESCE(am.total_ms, 0),
                release_date      = COALESCE(am.earliest_release, al.release_date),
                aggregates_updated_at = NOW()
            FROM affected aff
            LEFT JOIN album_meta am ON am.album_id = aff.album_id
            WHERE al.id = aff.album_id
            "#,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn refresh_album_popularity(&self) -> AppResult<()> {
        sqlx::query(
            r#"
            WITH album_plays AS (
                SELECT at.album_id, SUM(COALESCE(c.play_count, 0))::bigint AS plays
                FROM album_tracks at
                JOIN tracks it ON it.id = at.track_id
                LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                GROUP BY at.album_id
            ),
            denom AS (
                SELECT GREATEST(MAX(plays), 1)::bigint AS m FROM album_plays
            ),
            affected AS (
                SELECT album_id FROM album_plays WHERE plays > 0
                UNION
                SELECT id AS album_id FROM albums WHERE popularity_score > 0
            )
            UPDATE albums al SET
                popularity_score = LEAST(
                    1.0::real,
                    (LN(GREATEST(COALESCE(ap.plays, 0), 0) + 1)::real
                     / NULLIF(LN((SELECT m FROM denom) + 1)::real, 0))
                )
            FROM affected aff
            LEFT JOIN album_plays ap ON ap.album_id = aff.album_id
            WHERE al.id = aff.album_id
            "#,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    async fn refresh_album_star(&self) -> AppResult<()> {
        sqlx::query(
            r#"
            WITH affected AS (
                SELECT id AS album_id, primary_artist_id FROM albums
                WHERE primary_artist_id IS NOT NULL AND (
                    is_star_artist = TRUE
                    OR EXISTS (
                        SELECT 1 FROM artists a
                        WHERE a.id = albums.primary_artist_id AND a.is_star = TRUE
                    )
                )
            )
            UPDATE albums al SET
                is_star_artist = COALESCE(a.is_star, false)
            FROM affected aff
            LEFT JOIN artists a ON a.id = aff.primary_artist_id
            WHERE al.id = aff.album_id
            "#,
        )
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    pub async fn compute_summary(&self) -> AppResult<CachedSummary> {
        let (artists_count, albums_count, fresh_count): (i64, i64, i64) = sqlx::query_as(
            r#"SELECT
                COALESCE((SELECT reltuples::bigint FROM pg_class WHERE relname = 'artists'), 0)
                    - (SELECT COUNT(*)::bigint FROM artists WHERE merged_into IS NOT NULL),
                COALESCE((SELECT reltuples::bigint FROM pg_class WHERE relname = 'albums'), 0),
                (SELECT COUNT(*)::bigint FROM albums
                  WHERE release_date IS NOT NULL
                    AND release_date > (CURRENT_DATE - ($1::int * INTERVAL '1 day')))
            "#,
        )
        .bind(FRESH_WINDOW_DAYS)
        .fetch_one(&self.pg)
        .await?;

        Ok(CachedSummary {
            artists_count: artists_count.max(0),
            albums_count: albums_count.max(0),
            fresh_count,
            fresh_window_days: FRESH_WINDOW_DAYS,
        })
    }

    pub async fn compute_tag_list(&self) -> AppResult<CachedTagList> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            r#"SELECT g AS tag, COUNT(*)::bigint AS n
             FROM (
                 SELECT UNNEST(tags) AS g
                 FROM artists
                 WHERE merged_into IS NULL AND array_length(tags, 1) > 0
             ) t
             WHERE TRIM(g) <> ''
             GROUP BY g
             ORDER BY n DESC, g
             LIMIT $1"#,
        )
        .bind(TAG_PRECOMPUTE_LIMIT)
        .fetch_all(&self.pg)
        .await?;

        Ok(CachedTagList {
            items: rows
                .into_iter()
                .map(|(id, count)| CachedTag { id, count })
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
