use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use chrono::{NaiveDateTime, Utc};
use mini_moka::sync::Cache;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};
use crate::modules::auth::model::{LoginRequest, Session};
use crate::modules::oauth_apps::OAuthAppsService;
use crate::sc::{OAuthCredentials, ScClient, ScMe};

pub const REFRESH_BUFFER: Duration = Duration::from_secs(60);

const LOGIN_REQUEST_TTL_SECS: i64 = 15 * 60;
const REFRESH_LOCK_CAPACITY: u64 = 8192;
const REFRESH_LOCK_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, serde::Serialize)]
pub struct LoginInitResult {
    pub url: String,
    #[serde(rename = "loginRequestId")]
    pub login_request_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CallbackResult {
    pub login_request_id: Option<Uuid>,
    pub initial_status: String,
    pub username: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LoginStatusResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct AuthService {
    pool: PgPool,
    sc: ScClient,
    oauth_apps: Arc<OAuthAppsService>,
    config: Arc<AppConfig>,
    refresh_locks: Cache<Uuid, Arc<AsyncMutex<()>>>,
}

impl AuthService {
    pub fn new(
        pool: PgPool,
        sc: ScClient,
        oauth_apps: Arc<OAuthAppsService>,
        config: Arc<AppConfig>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pool,
            sc,
            oauth_apps,
            config,
            refresh_locks: Cache::builder()
                .max_capacity(REFRESH_LOCK_CAPACITY)
                .time_to_idle(REFRESH_LOCK_TTL)
                .build(),
        })
    }

    pub async fn get_session(&self, session_id: Uuid) -> AppResult<Option<Session>> {
        let row: Option<Session> = sqlx::query_as("SELECT * FROM sessions WHERE id = $1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    /// Возвращает сессию со свежим access token. Объединяет lookup + auto-refresh
    /// в один SQL round-trip на happy path (без refresh).
    pub async fn get_valid_session(&self, session_id: Uuid) -> AppResult<Session> {
        let session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| AppError::unauthorized("Session not found"))?;

        if !needs_refresh(&session.expires_at) {
            return Ok(session);
        }

        let lock = self.get_or_create_lock(session_id);
        let _g = lock.lock().await;

        let session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| AppError::unauthorized("Session not found"))?;
        if !needs_refresh(&session.expires_at) {
            return Ok(session);
        }

        self.do_refresh(session).await
    }

    pub async fn get_valid_access_token(&self, session_id: Uuid) -> AppResult<String> {
        Ok(self.get_valid_session(session_id).await?.access_token)
    }

    pub async fn refresh_session(&self, session_id: Uuid) -> AppResult<Session> {
        let lock = self.get_or_create_lock(session_id);
        let _g = lock.lock().await;
        let session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| AppError::unauthorized("Session not found"))?;
        self.do_refresh(session).await
    }

    async fn do_refresh(&self, session: Session) -> AppResult<Session> {
        if session.refresh_token.is_empty() {
            return Err(AppError::unauthorized("No refresh token available"));
        }
        let creds = self
            .get_credentials_for_app(session.oauth_app_id.as_deref())
            .await?;

        let token = match self
            .sc
            .refresh_access_token(&session.refresh_token, &creds)
            .await
        {
            Ok(t) => t,
            Err(err) => {
                warn!(session = %session.id, error = %err, "Refresh failed");
                return Err(AppError::unauthorized(
                    "Refresh token expired or invalid. Please re-authenticate.",
                ));
            }
        };

        let new_refresh = if token.refresh_token.is_empty() {
            session.refresh_token.clone()
        } else {
            token.refresh_token.clone()
        };
        let new_expires = (Utc::now() + chrono::Duration::seconds(token.expires_in)).naive_utc();
        let new_scope = if token.scope.is_empty() {
            session.scope.clone()
        } else {
            token.scope.clone()
        };

        let updated: Session = sqlx::query_as(
            "UPDATE sessions SET \
                access_token = $2, \
                refresh_token = $3, \
                expires_at = $4, \
                scope = $5, \
                updated_at = now() \
             WHERE id = $1 RETURNING *",
        )
        .bind(session.id)
        .bind(&token.access_token)
        .bind(&new_refresh)
        .bind(new_expires)
        .bind(&new_scope)
        .fetch_one(&self.pool)
        .await?;

        info!(session = %updated.id, "Session refreshed");
        Ok(updated)
    }

    pub async fn initiate_login(
        &self,
        existing_session_id: Option<Uuid>,
    ) -> AppResult<LoginInitResult> {
        let code_verifier = base64_url(&random_bytes(32));
        let code_challenge = base64_url(Sha256::digest(code_verifier.as_bytes()).as_slice());
        let state = hex::encode(random_bytes(16));

        let (creds, oauth_app_id) = match self.oauth_apps.pick_least_recently_used_app().await {
            Ok(app) => {
                info!(app_name = %app.name, app_id = %app.id, "Login initiated with app");
                let id = app.id;
                (
                    OAuthCredentials {
                        client_id: app.client_id,
                        client_secret: app.client_secret,
                        redirect_uri: app.redirect_uri,
                    },
                    Some(id.to_string()),
                )
            }
            Err(_) => {
                let env_creds = self.env_credentials();
                if env_creds.client_id.is_empty() || env_creds.client_secret.is_empty() {
                    return Err(AppError::not_found(
                        "No active OAuth apps available and env fallback is not configured",
                    ));
                }
                warn!("No active OAuth apps available, using env OAuth fallback");
                (env_creds, None)
            }
        };

        let target_session_id = match existing_session_id {
            Some(sid) => {
                let exists = self.get_session(sid).await?;
                if exists.is_some() {
                    info!(session = %sid, "Re-auth flow for existing session");
                    Some(sid)
                } else {
                    warn!(session = %sid, "Re-auth requested for unknown session, will create new");
                    None
                }
            }
            None => None,
        };

        let expires_at =
            (Utc::now() + chrono::Duration::seconds(LOGIN_REQUEST_TTL_SECS)).naive_utc();
        let login_request_id = Uuid::now_v7();

        sqlx::query(
            "INSERT INTO login_requests \
                (id, state, code_verifier, oauth_app_id, target_session_id, status, expires_at) \
             VALUES ($1, $2, $3, $4, $5, 'pending', $6)",
        )
        .bind(login_request_id)
        .bind(&state)
        .bind(&code_verifier)
        .bind(&oauth_app_id)
        .bind(target_session_id)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        let qs = serde_urlencoded::to_string([
            ("client_id", creds.client_id.as_str()),
            ("redirect_uri", creds.redirect_uri.as_str()),
            ("response_type", "code"),
            ("code_challenge", code_challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
        ])
        .map_err(|e| AppError::internal(format!("urlencode: {e}")))?;

        let url = format!("{}/authorize?{qs}", self.sc.auth_base_url());
        Ok(LoginInitResult {
            url,
            login_request_id,
        })
    }

    pub async fn handle_callback(
        self: &Arc<Self>,
        code: &str,
        state: &str,
    ) -> AppResult<CallbackResult> {
        let prefix_len = state.len().min(8);
        info!(state_prefix = %&state[..prefix_len], "Callback received");

        let claimed: Option<LoginRequest> = sqlx::query_as(
            "UPDATE login_requests SET status = 'processing', step = 'token' \
             WHERE state = $1 AND status = 'pending' RETURNING *",
        )
        .bind(state)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(lr) = claimed {
            let id = lr.id;
            let this = self.clone();
            let code = code.to_string();
            tokio::spawn(async move {
                this.run_callback_background(lr, code).await;
            });
            return Ok(CallbackResult {
                login_request_id: Some(id),
                initial_status: "pending".into(),
                username: None,
                error: None,
            });
        }

        let existing: Option<LoginRequest> =
            sqlx::query_as("SELECT * FROM login_requests WHERE state = $1")
                .bind(state)
                .fetch_optional(&self.pool)
                .await?;
        let Some(existing) = existing else {
            warn!("Callback state not found");
            return Ok(CallbackResult {
                login_request_id: None,
                initial_status: "failed".into(),
                username: None,
                error: Some(
                    "This login link is invalid or already used. Please try logging in again."
                        .into(),
                ),
            });
        };

        match existing.status.as_str() {
            "completed" => Ok(CallbackResult {
                login_request_id: Some(existing.id),
                initial_status: "completed".into(),
                username: existing.username,
                error: None,
            }),
            "processing" => Ok(CallbackResult {
                login_request_id: Some(existing.id),
                initial_status: "pending".into(),
                username: None,
                error: None,
            }),
            _ => Ok(CallbackResult {
                login_request_id: Some(existing.id),
                initial_status: "failed".into(),
                username: None,
                error: Some(
                    existing
                        .error
                        .unwrap_or_else(|| "This login link was already used.".into()),
                ),
            }),
        }
    }

    async fn run_callback_background(&self, lr: LoginRequest, code: String) {
        let id = lr.id;
        let result = self.do_callback_work(lr, code).await;
        if let Err(err) = result {
            error!(request = %id, error = %err, "Callback background failed");
            if let Err(e) = self.mark_request_failed(id, &err.to_string()).await {
                warn!(request = %id, error = %e, "Failed to mark request failed");
            }
        }
    }

    async fn do_callback_work(&self, lr: LoginRequest, code: String) -> AppResult<()> {
        let now = Utc::now().naive_utc();
        if lr.expires_at < now {
            self.mark_request_failed(lr.id, "Login request expired")
                .await?;
            return Ok(());
        }

        let creds = self
            .get_credentials_for_app(lr.oauth_app_id.as_deref())
            .await?;
        let token = match self
            .sc
            .exchange_code_for_token(&code, &lr.code_verifier, &creds)
            .await
        {
            Ok(t) => t,
            Err(err) => {
                warn!(request = %lr.id, error = %err, "Token exchange failed");
                let msg = public_error_message(&err, "Token exchange failed");
                self.mark_request_failed(lr.id, &msg).await?;
                return Ok(());
            }
        };

        if let Err(e) = sqlx::query("UPDATE login_requests SET step = 'profile' WHERE id = $1")
            .bind(lr.id)
            .execute(&self.pool)
            .await
        {
            warn!(request = %lr.id, error = %e, "Failed to advance step to profile");
        }

        let me = match self.fetch_sc_me_with_retries(&token.access_token).await {
            Some(me) => me,
            None => {
                self.mark_request_failed(lr.id, "Failed to fetch SoundCloud user info")
                    .await?;
                return Ok(());
            }
        };

        if let Err(e) = sqlx::query("UPDATE login_requests SET step = 'session' WHERE id = $1")
            .bind(lr.id)
            .execute(&self.pool)
            .await
        {
            warn!(request = %lr.id, error = %e, "Failed to advance step to session");
        }

        let expires_at = (Utc::now() + chrono::Duration::seconds(token.expires_in)).naive_utc();
        let scope = token.scope.clone();

        let session: Session = if let Some(target) = lr.target_session_id {
            let updated: Option<Session> = sqlx::query_as(
                "UPDATE sessions SET \
                    access_token = $2, refresh_token = $3, expires_at = $4, scope = $5, \
                    soundcloud_user_id = $6, username = $7, \
                    oauth_app_id = COALESCE($8, oauth_app_id), \
                    updated_at = now() \
                 WHERE id = $1 RETURNING *",
            )
            .bind(target)
            .bind(&token.access_token)
            .bind(&token.refresh_token)
            .bind(expires_at)
            .bind(&scope)
            .bind(&me.urn)
            .bind(&me.username)
            .bind(&lr.oauth_app_id)
            .fetch_optional(&self.pool)
            .await?;
            match updated {
                Some(s) => s,
                None => {
                    self.insert_session(&token, expires_at, &scope, &me, &lr.oauth_app_id)
                        .await?
                }
            }
        } else {
            self.insert_session(&token, expires_at, &scope, &me, &lr.oauth_app_id)
                .await?
        };

        sqlx::query(
            "UPDATE login_requests SET status = 'completed', step = NULL, \
                result_session_id = $2, username = $3 \
             WHERE id = $1",
        )
        .bind(lr.id)
        .bind(session.id)
        .bind(&me.username)
        .execute(&self.pool)
        .await?;

        info!(
            request = %lr.id,
            session = %session.id,
            user = ?me.username,
            "Login completed"
        );
        Ok(())
    }

    async fn insert_session(
        &self,
        token: &crate::sc::types::ScTokenResponse,
        expires_at: NaiveDateTime,
        scope: &str,
        me: &ScMe,
        oauth_app_id: &Option<String>,
    ) -> AppResult<Session> {
        let row: Session = sqlx::query_as(
            "INSERT INTO sessions \
                (id, access_token, refresh_token, expires_at, scope, \
                 soundcloud_user_id, username, oauth_app_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING *",
        )
        .bind(Uuid::now_v7())
        .bind(&token.access_token)
        .bind(&token.refresh_token)
        .bind(expires_at)
        .bind(scope)
        .bind(&me.urn)
        .bind(&me.username)
        .bind(oauth_app_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn mark_request_failed(&self, id: Uuid, err: &str) -> AppResult<()> {
        sqlx::query("UPDATE login_requests SET status = 'failed', error = $2 WHERE id = $1")
            .bind(id)
            .bind(err)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn fetch_sc_me_with_retries(&self, access_token: &str) -> Option<ScMe> {
        for attempt in 0..3 {
            match self.sc.api_get::<ScMe>("/me", access_token, None).await {
                Ok(me) => return Some(me),
                Err(AppError::ScApi { status, .. }) if status == 401 || status == 403 => {
                    error!(status = status, "Failed to fetch /me: auth error");
                    return None;
                }
                Err(err) => {
                    warn!(attempt, error = %err, "Failed to fetch /me, retrying");
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(200 * (attempt + 1))).await;
                    }
                }
            }
        }
        None
    }

    pub async fn get_login_request_status(
        &self,
        login_request_id: Uuid,
    ) -> AppResult<LoginStatusResult> {
        let row: Option<LoginRequest> =
            sqlx::query_as("SELECT * FROM login_requests WHERE id = $1")
                .bind(login_request_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(lr) = row else {
            return Ok(LoginStatusResult {
                status: "expired".into(),
                step: None,
                session_id: None,
                username: None,
                error: Some("Unknown login request".into()),
            });
        };

        let now = Utc::now().naive_utc();
        if (lr.status == "pending" || lr.status == "processing") && lr.expires_at < now {
            return Ok(LoginStatusResult {
                status: "expired".into(),
                step: None,
                session_id: None,
                username: None,
                error: Some("Login request expired".into()),
            });
        }

        let status = if lr.status == "processing" {
            "pending".to_string()
        } else {
            lr.status
        };

        Ok(LoginStatusResult {
            status,
            step: lr.step,
            session_id: lr.result_session_id,
            username: lr.username,
            error: lr.error,
        })
    }

    pub async fn logout(&self, session_id: Uuid) -> AppResult<()> {
        let Some(session) = self.get_session(session_id).await? else {
            return Ok(());
        };
        if !session.access_token.is_empty() {
            self.sc.sign_out(&session.access_token).await;
        }
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        self.refresh_locks.invalidate(&session_id);
        Ok(())
    }

    pub async fn cleanup_expired_login_requests(&self) -> AppResult<()> {
        let now = Utc::now().naive_utc();
        sqlx::query("DELETE FROM login_requests WHERE expires_at < $1")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn cleanup_expired_link_requests(&self) -> AppResult<()> {
        let now = Utc::now().naive_utc();
        sqlx::query("DELETE FROM link_requests WHERE expires_at < $1")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_credentials_for_app(
        &self,
        oauth_app_id: Option<&str>,
    ) -> AppResult<OAuthCredentials> {
        if let Some(id) = oauth_app_id {
            if let Some(app) = self.oauth_apps.get_by_id(id).await? {
                return Ok(OAuthCredentials {
                    client_id: app.client_id,
                    client_secret: app.client_secret,
                    redirect_uri: app.redirect_uri,
                });
            }
        }
        Ok(self.env_credentials())
    }

    fn env_credentials(&self) -> OAuthCredentials {
        OAuthCredentials {
            client_id: self.config.soundcloud.client_id.clone(),
            client_secret: self.config.soundcloud.client_secret.clone(),
            redirect_uri: self.config.soundcloud.redirect_uri.clone(),
        }
    }

    fn get_or_create_lock(&self, session_id: Uuid) -> Arc<AsyncMutex<()>> {
        if let Some(lock) = self.refresh_locks.get(&session_id) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        self.refresh_locks.insert(session_id, lock.clone());
        lock
    }
}

fn needs_refresh(expires_at: &NaiveDateTime) -> bool {
    let now = Utc::now().naive_utc();
    let buffer = chrono::Duration::seconds(REFRESH_BUFFER.as_secs() as i64);
    *expires_at - now <= buffer
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut buf);
    buf
}

fn base64_url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn public_error_message(err: &AppError, default: &str) -> String {
    match err {
        AppError::ScApi { body, .. } => {
            if let Some(desc) = body.get("error_description").and_then(|v| v.as_str()) {
                desc.to_string()
            } else if let Some(m) = body.get("message").and_then(|v| v.as_str()) {
                m.to_string()
            } else {
                default.to_string()
            }
        }
        other => {
            let s = other.to_string();
            if s.is_empty() {
                default.to_string()
            } else {
                s
            }
        }
    }
}
