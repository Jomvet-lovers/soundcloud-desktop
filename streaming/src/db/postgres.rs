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
    pub quality: String,
    pub cdn_path: Option<String>,
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

        let pg_pool = Self { pool };
        pg_pool.run_migrations().await?;
        Ok(pg_pool)
    }

    async fn run_migrations(&self) -> Result<(), PgError> {
        let client = self.pool.get().await?;

        // Add lastAccessedAt column to cdn_tracks if not exists
        // TypeORM uses camelCase column names by default
        client
            .execute(
                r#"
                DO $$
                BEGIN
                    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'cdn_tracks')
                       AND NOT EXISTS (
                           SELECT 1 FROM information_schema.columns
                           WHERE table_name = 'cdn_tracks' AND column_name = 'lastAccessedAt'
                       )
                    THEN
                        ALTER TABLE cdn_tracks
                        ADD COLUMN "lastAccessedAt" TIMESTAMPTZ DEFAULT NOW();
                    END IF;
                END $$;
                "#,
                &[],
            )
            .await?;

        info!("PG migrations done");
        Ok(())
    }

    /// Get session by x-session-id → access_token + soundcloud_user_id
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionInfo>, PgError> {
        let Ok(session_id) = Uuid::parse_str(session_id) else {
            return Ok(None);
        };
        let client = self.pool.get().await?;
        let row = client
            .query_opt(
                r#"SELECT "accessToken", "soundcloudUserId" FROM sessions WHERE id = $1"#,
                &[&session_id],
            )
            .await?;

        Ok(row.map(|r| SessionInfo {
            access_token: r.get(0),
            soundcloud_user_id: r.get(1),
        }))
    }

    /// Find cached CDN track (prefer HQ if specified)
    pub async fn find_cached_track(
        &self,
        track_urn: &str,
        prefer_hq: bool,
    ) -> Result<Option<CdnTrackRecord>, PgError> {
        let client = self.pool.get().await?;
        let rows = client
            .query(
                r#"SELECT id, "trackUrn", quality, "cdnPath", status
                   FROM cdn_tracks
                   WHERE "trackUrn" = $1 AND status = 'ok'"#,
                &[&track_urn],
            )
            .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        if prefer_hq {
            if let Some(hq) = rows.iter().find(|r| {
                let q: String = r.get(2);
                q == "hq"
            }) {
                return Ok(Some(row_to_cdn_track(hq)));
            }
        } else if let Some(sq) = rows.iter().find(|r| {
            let q: String = r.get(2);
            q == "sq"
        }) {
            return Ok(Some(row_to_cdn_track(sq)));
        }

        Ok(Some(row_to_cdn_track(&rows[0])))
    }

    /// Update last_accessed_at on CDN track
    pub async fn update_last_accessed(&self, id: &str) -> Result<(), PgError> {
        let client = self.pool.get().await?;
        client
            .execute(
                r#"UPDATE cdn_tracks SET "lastAccessedAt" = NOW() WHERE id = $1::text::uuid"#,
                &[&id],
            )
            .await?;
        Ok(())
    }

    /// Insert a new cdn_track record (upsert on conflict)
    pub async fn insert_cdn_track(
        &self,
        track_urn: &str,
        quality: &str,
        cdn_path: &str,
        status: &str,
    ) -> Result<String, PgError> {
        let id = Uuid::now_v7();
        let client = self.pool.get().await?;
        let row = client
            .query_one(
                r#"INSERT INTO cdn_tracks (id, "trackUrn", quality, "cdnPath", status, "createdAt", "updatedAt", "lastAccessedAt")
                   VALUES ($1::text::uuid, $2, $3, $4, $5, NOW(), NOW(), NOW())
                   ON CONFLICT ("trackUrn", quality) DO UPDATE
                   SET status = EXCLUDED.status,
                       "cdnPath" = EXCLUDED."cdnPath",
                       "updatedAt" = NOW()
                   RETURNING id"#,
                &[&id, &track_urn, &quality, &cdn_path, &status],
            )
            .await?;
        Ok(row.get::<_, Uuid>(0).to_string())
    }

    /// Update CDN track status
    pub async fn update_cdn_track_status(&self, id: &str, status: &str) -> Result<(), PgError> {
        let client = self.pool.get().await?;
        client
            .execute(
                r#"UPDATE cdn_tracks SET status = $2, "updatedAt" = NOW() WHERE id = $1::text::uuid"#,
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
                r#"SELECT id, "trackUrn", quality, "cdnPath", status
                   FROM cdn_tracks
                   WHERE status = 'ok'
                     AND "lastAccessedAt" < NOW() - $1::interval
                   ORDER BY "lastAccessedAt" ASC"#,
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
                r#"SELECT id, "trackUrn", quality, "cdnPath", status
                   FROM cdn_tracks
                   WHERE status = 'ok'
                   ORDER BY "lastAccessedAt" ASC
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
}

fn row_to_cdn_track(row: &tokio_postgres::Row) -> CdnTrackRecord {
    CdnTrackRecord {
        id: row.get::<_, Uuid>(0).to_string(),
        track_urn: row.get(1),
        quality: row.get(2),
        cdn_path: row.get(3),
        status: row.get(4),
    }
}
