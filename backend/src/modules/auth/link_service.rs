use std::sync::Arc;

use base64::Engine;
use chrono::{NaiveDateTime, Utc};
use rand::RngCore;
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::modules::auth::model::LinkRequestRow;
use crate::modules::auth::AuthService;

const LINK_REQUEST_TTL_SECS: i64 = 5 * 60;

pub struct LinkService {
    pool: PgPool,
    auth: Arc<AuthService>,
}

pub struct CreateLinkResult {
    pub link_request_id: Uuid,
    pub claim_token: String,
    pub expires_at: NaiveDateTime,
}

pub struct ClaimResult {
    pub session_id: Uuid,
    pub mode: String,
}

pub struct LinkStatusResult {
    pub status: String,
    pub mode: String,
    pub session_id: Option<Uuid>,
    pub error: Option<String>,
}

impl LinkService {
    pub fn new(pool: PgPool, auth: Arc<AuthService>) -> Arc<Self> {
        Arc::new(Self { pool, auth })
    }

    pub async fn create(
        &self,
        mode: &str,
        source_session_id: Option<Uuid>,
    ) -> AppResult<CreateLinkResult> {
        if mode != "pull" && mode != "push" {
            return Err(AppError::bad_request("mode must be 'pull' or 'push'"));
        }
        if mode == "push" && source_session_id.is_none() {
            return Err(AppError::bad_request("push mode requires source session"));
        }
        if mode == "pull" && source_session_id.is_some() {
            return Err(AppError::bad_request(
                "pull mode must not have source session at creation",
            ));
        }

        if let Some(src) = source_session_id {
            let exists = self.auth.get_session(src).await?;
            if exists.is_none() {
                return Err(AppError::unauthorized("Source session not found"));
            }
        }

        let mut token_bytes = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let claim_token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token_bytes);
        let expires_at =
            (Utc::now() + chrono::Duration::seconds(LINK_REQUEST_TTL_SECS)).naive_utc();

        let row: LinkRequestRow = sqlx::query_as(
            "INSERT INTO link_requests \
                (id, claim_token, mode, source_session_id, target_session_id, status, expires_at) \
             VALUES ($1, $2, $3, $4, NULL, 'pending', $5) RETURNING *",
        )
        .bind(Uuid::now_v7())
        .bind(&claim_token)
        .bind(mode)
        .bind(source_session_id)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await?;

        info!(id = %row.id, mode = %mode, "Link request created");
        Ok(CreateLinkResult {
            link_request_id: row.id,
            claim_token,
            expires_at: row.expires_at,
        })
    }

    pub async fn claim(
        &self,
        claim_token: &str,
        source_session_id_from_caller: Option<Uuid>,
    ) -> AppResult<ClaimResult> {
        let link: Option<LinkRequestRow> =
            sqlx::query_as("SELECT * FROM link_requests WHERE claim_token = $1")
                .bind(claim_token)
                .fetch_optional(&self.pool)
                .await?;
        let Some(link) = link else {
            return Err(AppError::not_found("Invalid or already used link token"));
        };

        if link.status != "pending" {
            return Err(AppError::bad_request(
                "Link token is already used or expired",
            ));
        }
        let now = Utc::now().naive_utc();
        if link.expires_at < now {
            sqlx::query("UPDATE link_requests SET status = 'failed', error = $2 WHERE id = $1")
                .bind(link.id)
                .bind("Expired")
                .execute(&self.pool)
                .await?;
            return Err(AppError::bad_request("Link token expired"));
        }

        let source_session_id = if link.mode == "pull" {
            source_session_id_from_caller
                .ok_or_else(|| AppError::unauthorized("pull claim requires source session"))?
        } else {
            link.source_session_id
                .ok_or_else(|| AppError::bad_request("push link has no source session"))?
        };

        let refreshed = match self.auth.get_valid_session(source_session_id).await {
            Ok(s) => s,
            Err(e) => {
                warn!(source = %source_session_id, error = %e, "Failed to refresh source session before link");
                return Err(AppError::unauthorized(
                    "Source session is not valid. The originating device must re-authenticate.",
                ));
            }
        };

        let target: crate::modules::auth::model::Session = sqlx::query_as(
            "INSERT INTO sessions \
                (id, access_token, refresh_token, expires_at, scope, \
                 soundcloud_user_id, username, oauth_app_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING *",
        )
        .bind(Uuid::now_v7())
        .bind(&refreshed.access_token)
        .bind(&refreshed.refresh_token)
        .bind(refreshed.expires_at)
        .bind(&refreshed.scope)
        .bind(&refreshed.soundcloud_user_id)
        .bind(&refreshed.username)
        .bind(&refreshed.oauth_app_id)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            "UPDATE link_requests SET source_session_id = $2, target_session_id = $3, status = 'claimed' \
             WHERE id = $1",
        )
        .bind(link.id)
        .bind(source_session_id)
        .bind(target.id)
        .execute(&self.pool)
        .await?;

        info!(
            id = %link.id,
            mode = %link.mode,
            source = %source_session_id,
            target = %target.id,
            "Link claimed"
        );
        Ok(ClaimResult {
            session_id: target.id,
            mode: link.mode,
        })
    }

    pub async fn get_status(&self, link_request_id: Uuid) -> AppResult<LinkStatusResult> {
        let row: Option<LinkRequestRow> =
            sqlx::query_as("SELECT * FROM link_requests WHERE id = $1")
                .bind(link_request_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(link) = row else {
            return Ok(LinkStatusResult {
                status: "expired".into(),
                mode: "pull".into(),
                session_id: None,
                error: Some("Unknown link request".into()),
            });
        };

        let now = Utc::now().naive_utc();
        if link.status == "pending" && link.expires_at < now {
            return Ok(LinkStatusResult {
                status: "expired".into(),
                mode: link.mode,
                session_id: None,
                error: Some("Expired".into()),
            });
        }

        let session_id = if link.mode == "pull" && link.status == "claimed" {
            link.target_session_id
        } else {
            None
        };

        Ok(LinkStatusResult {
            status: link.status,
            mode: link.mode,
            session_id,
            error: link.error,
        })
    }
}
