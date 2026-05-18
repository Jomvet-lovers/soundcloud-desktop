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
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*)::int8 FROM oauth_apps")
            .fetch_one(&self.pool)
            .await?;
        if total > 0 {
            return Ok(());
        }
        let sc = &self.config.soundcloud;
        if sc.client_id.is_empty() || sc.client_secret.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO oauth_apps (id, name, client_id, client_secret, redirect_uri, active) \
             VALUES ($1, $2, $3, $4, $5, true)",
        )
        .bind(Uuid::now_v7())
        .bind("default")
        .bind(&sc.client_id)
        .bind(&sc.client_secret)
        .bind(if sc.redirect_uri.is_empty() {
            "http://localhost:3000/auth/callback"
        } else {
            &sc.redirect_uri
        })
        .execute(&self.pool)
        .await?;
        info!("Migrated env OAuth credentials to oauth_apps table");
        Ok(())
    }

    pub async fn count_active(&self) -> AppResult<i64> {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::int8 FROM oauth_apps WHERE active = true")
                .fetch_one(&self.pool)
                .await?;
        Ok(n)
    }

    pub async fn pick_lru_from(&self, ids: &[Uuid]) -> AppResult<OAuthApp> {
        if ids.is_empty() {
            return Err(AppError::not_found("No OAuth apps in filter set"));
        }
        let mut tx = self.pool.begin().await?;
        let app: Option<OAuthApp> = sqlx::query_as(
            "SELECT * FROM oauth_apps \
             WHERE active = true AND id = ANY($1) \
             ORDER BY last_used_at ASC NULLS FIRST, created_at ASC \
             LIMIT 1 FOR UPDATE SKIP LOCKED",
        )
        .bind(ids)
        .fetch_optional(&mut *tx)
        .await?;

        let app = app.ok_or_else(|| AppError::not_found("No active OAuth apps available"))?;

        let updated: OAuthApp = sqlx::query_as(
            "UPDATE oauth_apps SET last_used_at = $1, updated_at = now() \
             WHERE id = $2 RETURNING *",
        )
        .bind(Utc::now())
        .bind(app.id)
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
        let row: Option<OAuthApp> = sqlx::query_as("SELECT * FROM oauth_apps WHERE id = $1")
            .bind(uuid)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    pub async fn find_all(&self) -> AppResult<Vec<OAuthApp>> {
        let rows: Vec<OAuthApp> =
            sqlx::query_as("SELECT * FROM oauth_apps ORDER BY created_at ASC")
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
        let row: OAuthApp = sqlx::query_as(
            "INSERT INTO oauth_apps (id, name, client_id, client_secret, redirect_uri, active) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
        )
        .bind(Uuid::now_v7())
        .bind(name)
        .bind(client_id)
        .bind(client_secret)
        .bind(redirect_uri)
        .bind(active.unwrap_or(true))
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
        let row: Option<OAuthApp> = sqlx::query_as(
            "UPDATE oauth_apps SET \
                name = COALESCE($2, name), \
                client_id = COALESCE($3, client_id), \
                client_secret = COALESCE($4, client_secret), \
                redirect_uri = COALESCE($5, redirect_uri), \
                active = COALESCE($6, active), \
                updated_at = now() \
             WHERE id = $1 RETURNING *",
        )
        .bind(uuid)
        .bind(name)
        .bind(client_id)
        .bind(client_secret)
        .bind(redirect_uri)
        .bind(active)
        .fetch_optional(&self.pool)
        .await?;
        row.ok_or_else(|| AppError::not_found("OAuth app not found"))
    }

    pub async fn remove(&self, id: &str) -> AppResult<()> {
        let uuid = match Uuid::parse_str(id) {
            Ok(u) => u,
            Err(_) => return Ok(()),
        };
        sqlx::query("DELETE FROM oauth_apps WHERE id = $1")
            .bind(uuid)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
