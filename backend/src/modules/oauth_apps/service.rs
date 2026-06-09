use std::sync::Arc;

use chrono::Utc;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::modules::oauth_apps::model::OAuthApp;

pub struct OAuthAppsService {
    pool: PgPool,
    config: Arc<AppConfig>,
}

impl OAuthAppsService {
    pub fn new(pool: PgPool, config: Arc<AppConfig>) -> Arc<Self> {
        Arc::new(Self { pool, config })
    }

    /// Разово: если таблица пустая — вставить env-кредов под именем `default`.
    pub async fn migrate_env_app(&self) -> AppResult<()> {
        let total: i64 = sqlx::query_file_scalar!("queries/oauth_apps/service/count_all.sql")
            .fetch_one(&self.pool)
            .await?;
        if total > 0 {
            return Ok(());
        }
        let sc = &self.config.soundcloud;
        if sc.client_id.is_empty() || sc.client_secret.is_empty() {
            return Ok(());
        }
        let redirect_uri = if sc.redirect_uri.is_empty() {
            "http://localhost:3000/auth/callback"
        } else {
            &sc.redirect_uri
        };
        sqlx::query_file!(
            "queries/oauth_apps/service/insert_env_app.sql",
            Uuid::now_v7(),
            "default",
            &sc.client_id,
            &sc.client_secret,
            redirect_uri
        )
        .execute(&self.pool)
        .await?;
        info!("Migrated env OAuth credentials to oauth_apps table");
        Ok(())
    }

    pub async fn count_active(&self) -> AppResult<i64> {
        let n: i64 = sqlx::query_file_scalar!("queries/oauth_apps/service/count_active.sql")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    pub async fn pick_lru_from(&self, ids: &[Uuid]) -> AppResult<OAuthApp> {
        if ids.is_empty() {
            return Err(AppError::not_found("No OAuth apps in filter set"));
        }
        let mut tx = self.pool.begin().await?;
        let app: Option<OAuthApp> = sqlx::query_file_as!(
            OAuthApp,
            "queries/oauth_apps/service/pick_lru_from.sql",
            ids
        )
        .fetch_optional(&mut *tx)
        .await?;

        let app = app.ok_or_else(|| AppError::not_found("No active OAuth apps available"))?;

        let updated: OAuthApp = sqlx::query_file_as!(
            OAuthApp,
            "queries/oauth_apps/service/touch_last_used.sql",
            Utc::now(),
            app.id
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        info!(app_name = %updated.name, app_id = %updated.id, "Picked OAuth app — lastUsedAt updated");
        Ok(updated)
    }

    pub async fn get_by_id(&self, id: &str) -> AppResult<Option<OAuthApp>> {
        let uuid = match Uuid::parse_str(id) {
            Ok(u) => u,
            Err(_) => return Ok(None),
        };
        let row: Option<OAuthApp> =
            sqlx::query_file_as!(OAuthApp, "queries/oauth_apps/service/get_by_id.sql", uuid)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row)
    }

    pub async fn find_all(&self) -> AppResult<Vec<OAuthApp>> {
        let rows: Vec<OAuthApp> =
            sqlx::query_file_as!(OAuthApp, "queries/oauth_apps/service/find_all.sql")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    pub async fn create(
        &self,
        name: &str,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
        active: Option<bool>,
    ) -> AppResult<OAuthApp> {
        let row: OAuthApp = sqlx::query_file_as!(
            OAuthApp,
            "queries/oauth_apps/service/create.sql",
            Uuid::now_v7(),
            name,
            client_id,
            client_secret,
            redirect_uri,
            active.unwrap_or(true)
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    pub async fn update(
        &self,
        id: &str,
        name: Option<&str>,
        client_id: Option<&str>,
        client_secret: Option<&str>,
        redirect_uri: Option<&str>,
        active: Option<bool>,
    ) -> AppResult<OAuthApp> {
        let uuid = Uuid::parse_str(id).map_err(|_| AppError::not_found("OAuth app not found"))?;
        let row: Option<OAuthApp> = sqlx::query_file_as!(
            OAuthApp,
            "queries/oauth_apps/service/update.sql",
            uuid,
            name,
            client_id,
            client_secret,
            redirect_uri,
            active
        )
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or_else(|| AppError::not_found("OAuth app not found"))
    }

    pub async fn remove(&self, id: &str) -> AppResult<()> {
        let uuid = match Uuid::parse_str(id) {
            Ok(u) => u,
            Err(_) => return Ok(()),
        };
        sqlx::query_file!("queries/oauth_apps/service/delete_by_id.sql", uuid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
