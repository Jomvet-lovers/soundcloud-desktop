use deadpool_postgres::{Config as PgConfig, Pool, Runtime};
use tokio_postgres::NoTls;
use tracing::info;
use uuid::Uuid;

use crate::config::Config;

#[derive(Debug, thiserror::Error)]
pub enum PgError {
    #[error("pool: {0}")]
    Pool(#[from] deadpool_postgres::PoolError),
    #[error("db: {0}")]
    Postgres(#[from] tokio_postgres::Error),
}

#[derive(Clone)]
pub struct PgPool {
    pool: Pool,
}

#[derive(Debug)]
pub struct SessionInfo {
    pub access_token: String,
    pub soundcloud_user_id: Option<String>,
}

#[derive(Debug)]
pub struct CdnTrackRecord {
    pub id: String,
    pub track_urn: String,
    pub status: String,
}

impl PgPool {
    pub async fn connect(config: &Config) -> Result<Self, Box<dyn std::error::Error>> {
        let mut pg = PgConfig::new();
        pg.host = Some(config.database_host.clone());
        pg.port = Some(config.database_port);
        pg.user = Some(config.database_username.clone());
        pg.password = Some(config.database_password.clone());
        pg.dbname = Some(config.database_name.clone());

        let pool = pg.create_pool(Some(Runtime::Tokio1), NoTls)?;

        // Test connection
        let client = pool.get().await?;
        client.execute("SELECT 1", &[]).await?;
        info!("PostgreSQL connected");

        Ok(Self { pool })
    }

    /// Get session by x-session-id → access_token + soundcloud_user_id
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>, PgError> {
        let Ok(session_id) = Uuid::parse_str(session_id) else {
            return Ok(None);
        };
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                r#"SELECT access_token, soundcloud_user_id FROM sessions WHERE id = $1"#,
                &[&session_id],
            )
            .await?;

        Ok(row.map(|r| SessionInfo {
            access_token: r.get(0),
            soundcloud_user_id: r.get(1),
        }))
    }

    /// Найти CDN-запись для трека (после m4a-перехода — одна на трек).
    /// Старые hq/sq-строки могут ещё быть в БД, но указывают на снесённые
    /// .ogg-файлы. Берём по `quality='single'`, чтобы их не задеть.
    pub async fn find_cached_track(
        &self,
        track_urn: &str,
    ) -> Result<Option<CdnTrackRecord>, PgError> {
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                r#"SELECT id, track_urn, status
                   FROM cdn_tracks
                   WHERE track_urn = $1 AND quality = 'single' AND status = 'ok'"#,
                &[&track_urn],
            )
            .await?;
        Ok(row.as_ref().map(row_to_cdn_track))
    }

    /// Update last_accessed_at on CDN track
    pub async fn update_last_accessed(&self, id: &str) -> Result<(), PgError> {
        let client = self.pool.get().await?;
        client
            .execute(
                r#"UPDATE cdn_tracks SET last_accessed_at = NOW() WHERE id = $1::text::uuid"#,
                &[&id],
            )
            .await?;
        Ok(())
    }

    /// Insert (upsert) the single cdn_track row for a track.
    /// Колонка `quality` сохранена для совместимости со старой схемой —
    /// всегда пишем `'single'`, чтобы не конфликтовать с легаси hq/sq-строками
    /// и попадать в уникальный индекс `(track_urn, quality)`.
    pub async fn insert_cdn_track(
        &self,
        track_urn: &str,
        cdn_path: &str,
        status: &str,
    ) -> Result<String, PgError> {
        let id = Uuid::now_v7().to_string();
        let quality = "single";
        let client = self.pool.get().await?;
        client
            .execute(
                r#"INSERT INTO cdn_tracks (id, track_urn, quality, cdn_path, status, created_at, updated_at, last_accessed_at)
                   VALUES ($1::text::uuid, $2, $3, $4, $5, NOW(), NOW(), NOW())
                   ON CONFLICT (track_urn, quality) DO UPDATE SET status = $5, cdn_path = $4, updated_at = NOW()"#,
                &[&id, &track_urn, &quality, &cdn_path, &status],
            )
            .await?;
        Ok(id)
    }

    /// Update CDN track status
    pub async fn update_cdn_track_status(&self, id: &str, status: &str) -> Result<(), PgError> {
        let client = self.pool.get().await?;
        client
            .execute(
                r#"UPDATE cdn_tracks SET status = $2, updated_at = NOW() WHERE id = $1::text::uuid"#,
                &[&id, &status],
            )
            .await?;
        Ok(())
    }

    /// Get stale CDN tracks for cleanup
    pub async fn get_stale_cdn_tracks(
        &self,
        older_than_days: u64,
    ) -> Result<Vec<CdnTrackRecord>, PgError> {
        let client = self.pool.get().await?;
        let interval = format!("{older_than_days} days");
        let rows = client
            .query(
                r#"SELECT id, track_urn, status
                   FROM cdn_tracks
                   WHERE status = 'ok'
                     AND last_accessed_at < NOW() - $1::interval
                   ORDER BY last_accessed_at ASC"#,
                &[&interval],
            )
            .await?;

        Ok(rows.iter().map(row_to_cdn_track).collect())
    }

    /// Get CDN tracks ordered by oldest access (for size-based cleanup)
    pub async fn get_cdn_tracks_oldest_first(
        &self,
        limit: i64,
    ) -> Result<Vec<CdnTrackRecord>, PgError> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                r#"SELECT id, track_urn, status
                   FROM cdn_tracks
                   WHERE status = 'ok'
                   ORDER BY last_accessed_at ASC
                   LIMIT $1"#,
                &[&limit],
            )
            .await?;

        Ok(rows.iter().map(row_to_cdn_track).collect())
    }

    /// Delete CDN track record
    pub async fn delete_cdn_track(&self, id: &str) -> Result<(), PgError> {
        let client = self.pool.get().await?;
        client
            .execute("DELETE FROM cdn_tracks WHERE id = $1::text::uuid", &[&id])
            .await?;
        Ok(())
    }

    /// Client-credentials токены из oauth_app_tokens (без user-сессий).
    /// Streaming юзает их как fallback, когда user-токен зарезан SC. Без
    /// FOR UPDATE — обычный SELECT, никаких lock'ов на hot-path streaming.
    pub async fn get_app_tokens(&self, exclude_token: &str) -> Result<Vec<String>, PgError> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                r#"SELECT access_token FROM oauth_app_tokens
                   WHERE expires_at > NOW() + INTERVAL '30 seconds'
                     AND access_token <> ''
                     AND access_token <> $1"#,
                &[&exclude_token],
            )
            .await?;
        let mut tokens: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
        use rand::seq::SliceRandom;
        tokens.shuffle(&mut rand::thread_rng());
        Ok(tokens)
    }

    /// Pickup кандидатов на HQ-upgrade. FOR UPDATE SKIP LOCKED + UPDATE
    /// hq_upgrade_last_at + attempts++ — два стриминга не возьмут одинаковый
    /// набор треков. retry_cooldown_sec задаёт паузу между попытками для
    /// одного и того же трека (защита от спама при недоступности hq).
    pub async fn pick_hq_upgrade_candidates(
        &self,
        limit: i64,
        retry_cooldown_sec: i64,
    ) -> Result<Vec<String>, PgError> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                r#"UPDATE tracks SET hq_upgrade_last_at = now(),
                       hq_upgrade_attempts = hq_upgrade_attempts + 1, updated_at = now()
                   WHERE id IN (
                       SELECT id FROM tracks
                       WHERE hq_upgrade_pending = true
                         AND (hq_upgrade_last_at IS NULL
                              OR hq_upgrade_last_at < now() - make_interval(secs => $2))
                       ORDER BY hq_upgrade_last_at NULLS FIRST, hq_upgrade_attempts
                       FOR UPDATE SKIP LOCKED
                       LIMIT $1
                   )
                   RETURNING urn"#,
                &[&limit, &(retry_cooldown_sec as f64)],
            )
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    pub async fn mark_hq_upgrade_failed(&self, urn: &str) -> Result<(), PgError> {
        let client = self.pool.get().await?;
        client
            .execute(
                "UPDATE tracks SET hq_upgrade_last_at = now(), updated_at = now() \
                 WHERE urn = $1",
                &[&urn],
            )
            .await?;
        Ok(())
    }

    /// Check if user has an active subscription
    pub async fn is_premium(&self, user_urn: &str) -> Result<bool, PgError> {
        let client = self.pool.get().await?;
        let now = chrono::Utc::now().timestamp();
        let row = client
            .query_opt(
                r#"SELECT 1 FROM subscriptions WHERE user_urn = $1 AND exp_date > $2"#,
                &[&user_urn, &now],
            )
            .await?;
        Ok(row.is_some())
    }
}

fn row_to_cdn_track(row: &tokio_postgres::Row) -> CdnTrackRecord {
    CdnTrackRecord {
        id: row.get::<_, Uuid>(0).to_string(),
        track_urn: row.get(1),
        status: row.get(2),
    }
}
