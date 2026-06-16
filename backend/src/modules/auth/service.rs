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
use crate::modules::auth::health::{AuthHealthService, RefreshFailKind};
use crate::modules::auth::model::{LoginRequest, Session};
use crate::modules::oauth_apps::model::OAuthApp;
use crate::modules::oauth_apps::OAuthAppsService;
use crate::sc::Apiv2Proxy;
use crate::sc::{self, OAuthCredentials, ScClient, ScMe};
use serde_json::Value;

/// За сколько до экспайра считаем токен «пора рефрешить». Широкий буфер =
/// renew-on-open: открывший аппу юзер получает токен на всю сессию, а не
/// рефреш за 60с до протухания посреди работы. Рефреш всё равно условный.
pub const REFRESH_BUFFER: Duration = Duration::from_secs(5 * 60);

const LOGIN_REQUEST_TTL_SECS: i64 = 15 * 60;
const MAX_AUTH_RETRIES: i32 = 3;
const PROFILE_TIMEOUT_SEC: u64 = 5;
const REFRESH_LOCK_CAPACITY: u64 = 8192;
const REFRESH_LOCK_TTL: Duration = Duration::from_secs(10 * 60);
/// Мягкий потолок выпуска токенов на app за 12ч (SC-лимит 50/12ч/app) — новые
/// логины предпочитают apps под этим порогом. Рефреши привязаны к issuing-app.
const PER_APP_ISSUE_SOFT_CAP: i64 = 45;

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
    #[serde(rename = "redirectUrl", skip_serializing_if = "Option::is_none")]
    pub redirect_url: Option<String>,
    /// Result of the best-effort profile extraction: "ok" | "failed" | None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract: Option<String>,
}

pub struct AuthService {
    pool: PgPool,
    sc: ScClient,
    anon: Apiv2Proxy,
    oauth_apps: Arc<OAuthAppsService>,
    config: Arc<AppConfig>,
    health: Arc<AuthHealthService>,
    refresh_locks: Cache<Uuid, Arc<AsyncMutex<()>>>,
}

impl AuthService {
    pub fn new(
        pool: PgPool,
        sc: ScClient,
        oauth_apps: Arc<OAuthAppsService>,
        config: Arc<AppConfig>,
        health: Arc<AuthHealthService>,
    ) -> Arc<Self> {
        let anon = Apiv2Proxy::new(sc.clone());
        Arc::new(Self {
            pool,
            sc,
            anon,
            oauth_apps,
            config,
            health,
            refresh_locks: Cache::builder()
                .max_capacity(REFRESH_LOCK_CAPACITY)
                .time_to_idle(REFRESH_LOCK_TTL)
                .build(),
        })
    }

    pub async fn get_session(&self, session_id: Uuid) -> AppResult<Option<Session>> {
        let row = sqlx::query_file_as!(Session, "queries/auth/service/get_session.sql", session_id)
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

    /// Подбирает свежую сессию по sc_user_id (юзер может быть залогинен с нескольких
    /// устройств) и возвращает валидный access_token. Нужен sync-воркеру: action в
    /// очереди привязан к пользователю, а не к конкретной сессии.
    pub async fn get_valid_access_token_for_user(&self, sc_user_id: &str) -> AppResult<String> {
        // sync_queue.user_id канонизирован в bare, а sessions.soundcloud_user_id —
        // JWT sub (URN). Матчим по обоим вариантам, иначе воркер не найдёт токен
        // и все queued-действия отвалятся (обязательно в одном релизе с каноном).
        let variants = crate::common::sc_ids::user_id_variants(sc_user_id);
        let session_id = sqlx::query_file_scalar!(
            "queries/auth/service/pick_session_id_for_user.sql",
            &variants
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| AppError::unauthorized("No active session for user"))?;
        self.get_valid_access_token(session_id).await
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

        // Circuit breaker: если этот session недавно зафейлился — не идём в SC
        // снова (защита от retry-storm на стороне фронта/прокси, который дёргает
        // /refresh на каждую 401-ошибку). TTL ключа = REFRESH_FAIL_TTL_SEC.
        let session_key = session.id.to_string();
        if let Ok(Some((kind, msg))) = self.health.get_cached_refresh_failure(&session_key).await {
            return Err(refresh_err(kind, msg));
        }

        let creds = self
            .get_credentials_for_app(session.oauth_app_id.as_deref())
            .await?;

        let token = match self
            .sc
            .refresh_access_token(&session.refresh_token, &creds)
            .await
        {
            Ok(t) => {
                if let Some(app_id) = session.oauth_app_id.as_deref() {
                    let _ = self.health.record_app_success(app_id).await;
                    let _ = self.health.record_token_issue(app_id).await;
                }
                let _ = self.health.clear_refresh_failure(&session_key).await;
                t
            }
            Err(err) => {
                // rate-limit / явный отказ гранта (re-auth) / транзиент (роут
                // лёг — НЕ перелогинивать, тихо ретраить). Дефолт — транзиент:
                // неизвестная ошибка НЕ должна выкидывать юзера на ре-логин.
                let (kind, user_msg) = if sc::is_rate_limited(&err) {
                    (
                        RefreshFailKind::RateLimit,
                        "SoundCloud rate-limited the refresh. Try again in a few minutes."
                            .to_string(),
                    )
                } else if sc::is_invalid_grant(&err) {
                    (
                        RefreshFailKind::ReAuth,
                        "Session expired. Please sign in again.".to_string(),
                    )
                } else {
                    (
                        RefreshFailKind::Transient,
                        "Renewing your session, try again shortly.".to_string(),
                    )
                };
                let _ = self
                    .health
                    .cache_refresh_failure(&session_key, &user_msg, kind)
                    .await;
                if let Some(app_id) = session.oauth_app_id.as_deref() {
                    let _ = self.health.record_app_failure(app_id).await;
                }
                warn!(session = %session.id, error = %err, ?kind, "Refresh failed");
                return Err(refresh_err(kind, user_msg));
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

        let updated = sqlx::query_file_as!(
            Session,
            "queries/auth/service/refresh_session_update.sql",
            session.id,
            token.access_token,
            new_refresh,
            new_expires,
            new_scope,
        )
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

        let (creds, oauth_app_id) = self.pick_credentials(None).await?;

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

        let url = self.build_authorize_url(&creds, &state, &code_challenge)?;
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

        let claimed = sqlx::query_file_as!(
            LoginRequest,
            "queries/auth/service/claim_login_request.sql",
            state
        )
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

        let existing = sqlx::query_file_as!(
            LoginRequest,
            "queries/auth/service/get_login_request_by_state.sql",
            state
        )
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
            Ok(t) => {
                if let Some(app_id) = lr.oauth_app_id.as_deref() {
                    let _ = self.health.record_app_success(app_id).await;
                    let _ = self.health.record_token_issue(app_id).await;
                }
                t
            }
            Err(err) => {
                warn!(request = %lr.id, error = %err, "Token exchange failed");
                let msg = public_error_message(&err, "Token exchange failed");
                self.retry_with_new_app(&lr, &msg).await?;
                return Ok(());
            }
        };

        if let Err(e) =
            sqlx::query_file!("queries/auth/service/login_request_step_extract.sql", lr.id)
                .execute(&self.pool)
                .await
        {
            warn!(request = %lr.id, error = %e, "Failed to advance step to extract");
        }

        // Identify the user from the access-token JWT `sub` (authoritative, no
        // network). Fall back to /me only if the token is not a readable JWT.
        let mut profile: Option<Value> = None;
        let urn = match urn_from_access_token(&token.access_token) {
            Some(u) if !u.is_empty() => u,
            _ => match self.fetch_sc_me(&token.access_token).await {
                MeOutcome::Found(me) => {
                    let u = me.urn.clone();
                    profile = serde_json::to_value(&me).ok();
                    u
                }
                MeOutcome::Unauthorized => {
                    self.retry_with_new_app(&lr, "SoundCloud rejected the token")
                        .await?;
                    return Ok(());
                }
                MeOutcome::Unreachable => {
                    self.retry_with_new_app(&lr, "Failed to identify SoundCloud user")
                        .await?;
                    return Ok(());
                }
            },
        };

        // Best-effort avatar/username: race the working profile sources
        // (v1 /me + anon v2 /users/{id}), bounded; never blocks login.
        if profile.is_none() {
            let nid = urn.rsplit(':').next().unwrap_or("");
            profile = self.fetch_profile_fast(&token.access_token, nid).await;
        }
        let username: Option<String> = profile
            .as_ref()
            .and_then(|p| p.get("username").and_then(|v| v.as_str()))
            .map(|s| s.to_string());
        let profile_ok = profile.is_some();
        if let Some(ref p) = profile {
            let _ = sqlx::query_file!("queries/auth/service/upsert_user_profile.sql", &urn, p)
                .execute(&self.pool)
                .await;
        }

        if let Err(e) = sqlx::query_file!(
            "queries/auth/service/login_request_step_finalizing.sql",
            lr.id
        )
        .execute(&self.pool)
        .await
        {
            warn!(request = %lr.id, error = %e, "Failed to advance step to finalizing");
        }

        // Prefer the JWT `exp` (authoritative) for session expiry; fall back to
        // the token-response `expires_in`.
        let expires_at = exp_from_access_token(&token.access_token)
            .and_then(|e| chrono::DateTime::from_timestamp(e, 0))
            .map(|dt| dt.naive_utc())
            .unwrap_or_else(|| {
                (Utc::now() + chrono::Duration::seconds(token.expires_in)).naive_utc()
            });
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
            .bind(&urn)
            .bind(&username)
            .bind(&lr.oauth_app_id)
            .fetch_optional(&self.pool)
            .await?;
            match updated {
                Some(s) => s,
                None => {
                    self.insert_session(
                        &token,
                        expires_at,
                        &scope,
                        &urn,
                        username.as_deref(),
                        &lr.oauth_app_id,
                    )
                    .await?
                }
            }
        } else {
            self.insert_session(
                &token,
                expires_at,
                &scope,
                &urn,
                username.as_deref(),
                &lr.oauth_app_id,
            )
            .await?
        };

        sqlx::query_file!(
            "queries/auth/service/login_request_completed.sql",
            lr.id,
            session.id,
            username,
            profile_ok
        )
        .execute(&self.pool)
        .await?;

        info!(
            request = %lr.id,
            session = %session.id,
            user = ?username,
            "Login completed"
        );
        Ok(())
    }

    async fn insert_session(
        &self,
        token: &crate::sc::types::ScTokenResponse,
        expires_at: NaiveDateTime,
        scope: &str,
        urn: &str,
        username: Option<&str>,
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
        .bind(urn)
        .bind(username)
        .bind(oauth_app_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    async fn mark_request_failed(&self, id: Uuid, err: &str) -> AppResult<()> {
        sqlx::query_file!("queries/auth/service/login_request_failed.sql", id, err)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn fetch_sc_me(&self, access_token: &str) -> MeOutcome {
        for attempt in 0..3 {
            match self.sc.api_get::<ScMe>("/me", access_token, None).await {
                Ok(me) => return MeOutcome::Found(me),
                Err(AppError::ScApi { status, .. }) if status == 401 || status == 403 => {
                    error!(status = status, "Failed to fetch /me: auth error");
                    return MeOutcome::Unauthorized;
                }
                Err(err) => {
                    warn!(attempt, error = %err, "Failed to fetch /me, retrying");
                    if attempt < 2 {
                        tokio::time::sleep(Duration::from_millis(200 * (attempt + 1))).await;
                    }
                }
            }
        }
        MeOutcome::Unreachable
    }

    /// Best-effort profile (avatar/username): race v1 /me (user token) against
    /// anon v2 /users/{id} (scraped client_id) and take the first success within
    /// a bound. Never blocks login — returns None if both are slow/unavailable.
    async fn fetch_profile_fast(&self, token: &str, nid: &str) -> Option<Value> {
        let me_fut: std::pin::Pin<
            Box<dyn std::future::Future<Output = AppResult<Value>> + Send + '_>,
        > = Box::pin(self.sc.api_get::<Value>("/me", token, None));
        let anon_fut: std::pin::Pin<
            Box<dyn std::future::Future<Output = AppResult<Value>> + Send + '_>,
        > = Box::pin(self.anon.user(nid));
        match tokio::time::timeout(
            Duration::from_secs(PROFILE_TIMEOUT_SEC),
            futures::future::select_ok(vec![me_fut, anon_fut]),
        )
        .await
        {
            Ok(Ok((v, _rest))) => Some(v),
            _ => None,
        }
    }

    pub async fn get_login_request_status(
        &self,
        login_request_id: Uuid,
    ) -> AppResult<LoginStatusResult> {
        let row = sqlx::query_file_as!(
            LoginRequest,
            "queries/auth/service/get_login_request.sql",
            login_request_id
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(lr) = row else {
            return Ok(LoginStatusResult {
                status: "expired".into(),
                step: None,
                session_id: None,
                username: None,
                error: Some("Unknown login request".into()),
                redirect_url: None,
                extract: None,
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
                redirect_url: None,
                extract: None,
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
            redirect_url: lr.redirect_url,
            extract: lr.profile_ok.map(|ok| {
                if ok {
                    "ok".to_string()
                } else {
                    "failed".to_string()
                }
            }),
        })
    }

    pub async fn logout(&self, session_id: Uuid) -> AppResult<()> {
        let Some(session) = self.get_session(session_id).await? else {
            return Ok(());
        };
        if !session.access_token.is_empty() {
            self.sc.sign_out(&session.access_token).await;
        }
        sqlx::query_file!("queries/auth/service/delete_session.sql", session_id)
            .execute(&self.pool)
            .await?;
        self.refresh_locks.invalidate(&session_id);
        Ok(())
    }

    pub async fn cleanup_expired_login_requests(&self) -> AppResult<()> {
        let now = Utc::now().naive_utc();
        sqlx::query_file!(
            "queries/auth/service/delete_expired_login_requests.sql",
            now
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cleanup_expired_link_requests(&self) -> AppResult<()> {
        let now = Utc::now().naive_utc();
        sqlx::query_file!("queries/auth/service/delete_expired_link_requests.sql", now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Реапер мёртвых сессий: (1) истёкшие и нетронутые >7д; (2) истёкшие
    /// дубли (оставляем свежайшую на `soundcloud_user_id`). Живые и недавно-
    /// активные истёкшие НЕ трогаем — их оживит renew-on-open.
    pub async fn reap_dead_sessions(&self) -> AppResult<()> {
        let stale = sqlx::query_file!("queries/auth/service/reap_stale_sessions.sql")
            .execute(&self.pool)
            .await?
            .rows_affected();

        let dupes = sqlx::query_file!("queries/auth/service/reap_duplicate_sessions.sql")
            .execute(&self.pool)
            .await?
            .rows_affected();

        if stale > 0 || dupes > 0 {
            info!(stale, dupes, "reaped dead sessions");
        }
        Ok(())
    }

    async fn pick_healthy_app(&self, exclude: Option<Uuid>) -> AppResult<OAuthApp> {
        let all = self.oauth_apps.find_all().await?;
        let active: Vec<OAuthApp> = all
            .into_iter()
            .filter(|a| a.active && Some(a.id) != exclude)
            .collect();
        if active.is_empty() {
            return Err(AppError::not_found("No active OAuth apps available"));
        }

        let ids: Vec<String> = active.iter().map(|a| a.id.to_string()).collect();
        let healths = self.health.app_healths(&ids).await.unwrap_or_default();
        let penalties = self.health.app_penalties(&ids).await.unwrap_or_default();
        let issued = self
            .health
            .tokens_issued_12h(&ids)
            .await
            .unwrap_or_default();

        // Предпочитаем чистые apps под per-app 12ч-бюджетом; если таких нет —
        // деградируем без бюджет-фильтра (он мягкий — рефреши всё равно
        // привязаны к issuing-app и не редистрибутируются).
        let pick = |require_budget: bool| -> Vec<Uuid> {
            active
                .iter()
                .filter(|a| {
                    let key = a.id.to_string();
                    let healthy = healths.get(&key).map(|h| !h.unhealthy()).unwrap_or(true);
                    let clean = healthy && !penalties.contains_key(&key);
                    let budget_ok = !require_budget
                        || issued.get(&key).copied().unwrap_or(0) < PER_APP_ISSUE_SOFT_CAP;
                    clean && budget_ok
                })
                .map(|a| a.id)
                .collect()
        };
        let mut preferred = pick(true);
        if preferred.is_empty() {
            preferred = pick(false);
        }
        if !preferred.is_empty() {
            return self.oauth_apps.pick_lru_from(&preferred).await;
        }

        warn!("No clean OAuth app available; degrading to least-penalized pick");
        let mut by_penalty: Vec<&OAuthApp> = active.iter().collect();
        by_penalty.sort_by_key(|a| penalties.get(&a.id.to_string()).copied().unwrap_or(0));
        let ids: Vec<Uuid> = by_penalty.iter().map(|a| a.id).collect();
        self.oauth_apps.pick_lru_from(&ids).await
    }

    async fn pick_credentials(
        &self,
        exclude: Option<Uuid>,
    ) -> AppResult<(OAuthCredentials, Option<String>)> {
        match self.pick_healthy_app(exclude).await {
            Ok(app) => {
                info!(app_name = %app.name, app_id = %app.id, "Auth flow using app");
                let id = app.id;
                Ok((
                    OAuthCredentials {
                        client_id: app.client_id,
                        client_secret: app.client_secret,
                        redirect_uri: app.redirect_uri,
                    },
                    Some(id.to_string()),
                ))
            }
            Err(_) => {
                let env_creds = self.env_credentials();
                if env_creds.client_id.is_empty() || env_creds.client_secret.is_empty() {
                    return Err(AppError::not_found(
                        "No active OAuth apps available and env fallback is not configured",
                    ));
                }
                warn!("No active OAuth apps available, using env OAuth fallback");
                Ok((env_creds, None))
            }
        }
    }

    fn build_authorize_url(
        &self,
        creds: &OAuthCredentials,
        state: &str,
        code_challenge: &str,
    ) -> AppResult<String> {
        let qs = serde_urlencoded::to_string([
            ("client_id", creds.client_id.as_str()),
            ("redirect_uri", creds.redirect_uri.as_str()),
            ("response_type", "code"),
            ("code_challenge", code_challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
        ])
        .map_err(|e| AppError::internal(format!("urlencode: {e}")))?;
        Ok(format!("{}/authorize?{qs}", self.sc.auth_base_url()))
    }

    async fn retry_with_new_app(&self, lr: &LoginRequest, reason: &str) -> AppResult<()> {
        if let Some(app_id) = lr.oauth_app_id.as_deref() {
            let _ = self.health.record_app_failure(app_id).await;
            match self.health.penalize_app(app_id).await {
                Ok(cd) => warn!(app_id, cooldown_sec = cd, %reason, "OAuth app penalized"),
                Err(e) => warn!(app_id, error = %e, "Failed to penalize app"),
            }
        }

        if lr.retry_count >= MAX_AUTH_RETRIES {
            warn!(request = %lr.id, retries = lr.retry_count, "Auth retries exhausted");
            self.mark_request_failed(lr.id, reason).await?;
            return Ok(());
        }

        let exclude = lr
            .oauth_app_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok());
        let (creds, new_app_id) = match self.pick_credentials(exclude).await {
            Ok(v) => v,
            Err(_) => {
                self.mark_request_failed(lr.id, reason).await?;
                return Ok(());
            }
        };

        let code_verifier = base64_url(&random_bytes(32));
        let code_challenge = base64_url(Sha256::digest(code_verifier.as_bytes()).as_slice());
        let state = hex::encode(random_bytes(16));
        let url = self.build_authorize_url(&creds, &state, &code_challenge)?;
        let expires_at =
            (Utc::now() + chrono::Duration::seconds(LOGIN_REQUEST_TTL_SECS)).naive_utc();

        sqlx::query(
            "UPDATE login_requests SET \
                status = 'pending', step = NULL, error = NULL, \
                state = $2, code_verifier = $3, oauth_app_id = $4, \
                redirect_url = $5, retry_count = retry_count + 1, \
                expires_at = $6 \
             WHERE id = $1",
        )
        .bind(lr.id)
        .bind(&state)
        .bind(&code_verifier)
        .bind(&new_app_id)
        .bind(&url)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        info!(
            request = %lr.id,
            attempt = lr.retry_count + 1,
            new_app = ?new_app_id,
            "Auth retried with a different OAuth app"
        );
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

/// Вид фейла рефреша → AppError с правильным HTTP-статусом для фронта:
/// transient→502 (тихий ретрай), rate→429 (без модалки), re-auth→401 (модалка).
fn refresh_err(kind: RefreshFailKind, msg: String) -> AppError {
    match kind {
        RefreshFailKind::ReAuth => AppError::unauthorized(msg),
        RefreshFailKind::RateLimit => AppError::ScApi {
            status: 429,
            body: serde_json::json!({ "message": msg }),
        },
        RefreshFailKind::Transient => AppError::ScUnreachable(msg),
    }
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

enum MeOutcome {
    Found(ScMe),
    Unauthorized,
    Unreachable,
}

/// Extract the SoundCloud user urn from the access token's JWT `sub` claim. The
/// token is minted by SC's token endpoint (trusted by provenance), so the urn is
/// authoritative without a /me round-trip. Returns None if the token is not a
/// readable JWT.
fn urn_from_access_token(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let sub = claims.get("sub")?.as_str()?.trim();
    if sub.is_empty() {
        None
    } else {
        Some(sub.to_string())
    }
}

/// Extract the token expiry (`exp`, unix seconds) from the access-token JWT.
fn exp_from_access_token(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims.get("exp")?.as_i64()
}
