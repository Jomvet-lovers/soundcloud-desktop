//! Пул `client_credentials`-токенов под public-операции SC (search/resolve/
//! public reads). Hot-path lock-free для чтения: токены лежат в
//! `RwLock<Arc<Vec<String>>>`, `snapshot()` берёт read-lock на наносекунды и
//! клонирует Arc'у — без DB и без write-блокировок.
//!
//! Источник истины — таблица `oauth_app_tokens`: cron-tick рефрешит просроченные
//! и атомарно перезаливает snapshot. Первый `snapshot()` после старта однократно
//! подтянет всё из БД (через `OnceCell`).
//!
//! SC rate limits:
//!   * 50 токенов / 12h / app — один токен на аппку, рефреш по `expires_in`.
//!   * 30 токенов / 1h / IP   — между аппками в одном тике пауза `REFRESH_GAP`.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::seq::SliceRandom;
use sqlx::PgPool;
use tokio::sync::{Mutex, OnceCell};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::modules::oauth_apps::OAuthAppsService;
use crate::sc::ScClient;

/// За сколько до `expires_at` начинаем превентивный рефреш токена.
const REFRESH_LEAD: chrono::Duration = chrono::Duration::seconds(5 * 60);
/// Минимальный остаток валидности, при котором токен ещё попадает в snapshot.
const MIN_FRESH: chrono::Duration = chrono::Duration::seconds(30);
/// Период cron-tick'а проверки истекающих токенов.
const REFRESH_TICK: Duration = Duration::from_secs(60);
/// Пауза между рефрешами разных аппок в одном тике (защита 30/1h/IP burst).
const REFRESH_GAP: Duration = Duration::from_millis(1500);
/// Сколько последовательных фейлов рефреша терпим — после circuit-breaker до
/// следующего successful refresh'а.
const MAX_REFRESH_ATTEMPTS: i32 = 5;
/// После исчерпания `MAX_REFRESH_ATTEMPTS` app паркуется; повтор разрешаем
/// спустя этот кулдаун. Транзиентный аутэйдж SC-роута не должен убивать app
/// навсегда — именно это (без сброса счётчика) положило 12/13 app-токенов.
const RETRY_COOLDOWN_SECS: i64 = 15 * 60;

pub struct OAuthAppTokenService {
    pg: PgPool,
    sc: ScClient,
    apps: Arc<OAuthAppsService>,
    snapshot: RwLock<Arc<Vec<String>>>,
    seeded: OnceCell<()>,
    refresh_lock: Mutex<()>,
    bootstrap_lock: Mutex<()>,
}

impl OAuthAppTokenService {
    pub fn new(pg: PgPool, sc: ScClient, apps: Arc<OAuthAppsService>) -> Arc<Self> {
        Arc::new(Self {
            pg,
            sc,
            apps,
            snapshot: RwLock::new(Arc::new(Vec::new())),
            seeded: OnceCell::new(),
            refresh_lock: Mutex::new(()),
            bootstrap_lock: Mutex::new(()),
        })
    }

    /// Перемешанный список всех живых токенов пула. Без DB-запросов и без
    /// write-локов на hot-path. Если snapshot пуст (cold start) — однократно
    /// подтянет из БД и при необходимости синхронно сделает refresh первой
    /// аппки.
    pub async fn snapshot(&self) -> AppResult<Vec<String>> {
        self.seeded
            .get_or_try_init(|| async {
                self.reload_snapshot().await?;
                if self.load_current().is_empty() {
                    self.bootstrap_first_app().await.ok();
                    self.reload_snapshot().await?;
                }
                Ok::<_, AppError>(())
            })
            .await?;

        let cur = self.load_current();
        if !cur.is_empty() {
            return Ok(shuffled(&cur));
        }
        // Snapshot мог опустеть между seed'ом и сейчас (TTL у всех токенов
        // протухло, cron ещё не успел). Принудительно прогреваем.
        self.bootstrap_first_app().await?;
        self.reload_snapshot().await?;
        let cur = self.load_current();
        if cur.is_empty() {
            return Err(AppError::internal(
                "public-token pool empty after bootstrap",
            ));
        }
        Ok(shuffled(&cur))
    }

    fn load_current(&self) -> Arc<Vec<String>> {
        self.snapshot
            .read()
            .map(|g| Arc::clone(&g))
            .unwrap_or_else(|_| Arc::new(Vec::new()))
    }

    async fn reload_snapshot(&self) -> AppResult<()> {
        let cutoff = Utc::now() + MIN_FRESH;
        let tokens: Vec<String> = sqlx::query_file_scalar!(
            "queries/oauth_apps/token_service/reload_snapshot.sql",
            cutoff
        )
        .fetch_all(&self.pg)
        .await?;
        if let Ok(mut g) = self.snapshot.write() {
            *g = Arc::new(tokens);
        }
        Ok(())
    }

    async fn bootstrap_first_app(&self) -> AppResult<()> {
        // Single-flight рефилл пустого пула: конкурентные hot-path вызовы
        // сериализуются здесь, пир уже мог дозалить — перечитываем и выходим,
        // не issue'я лишний oauth/token POST (бережём лимит 30/1h/IP).
        let _guard = self.bootstrap_lock.lock().await;
        self.reload_snapshot().await?;
        if !self.load_current().is_empty() {
            return Ok(());
        }
        let app_id = self.next_app_to_refresh().await?;
        self.refresh_for_app(app_id).await
    }

    async fn next_app_to_refresh(&self) -> AppResult<Uuid> {
        let mut ranked: Vec<(Uuid, Option<DateTime<Utc>>, i32)> = Vec::new();
        for app in self.apps.find_all().await?.into_iter().filter(|a| a.active) {
            let row = sqlx::query_file!(
                "queries/oauth_apps/token_service/find_app_token_state.sql",
                app.id
            )
            .fetch_optional(&self.pg)
            .await?;
            let (exp, attempts) = row
                .map(|r| (Some(r.expires_at), r.refresh_attempts))
                .unwrap_or((None, 0));
            if attempts >= MAX_REFRESH_ATTEMPTS {
                continue;
            }
            ranked.push((app.id, exp, attempts));
        }
        ranked.sort_by_key(|(_, exp, _)| {
            exp.unwrap_or(DateTime::<Utc>::from_timestamp(0, 0).unwrap())
        });
        ranked
            .first()
            .map(|(id, _, _)| *id)
            .ok_or_else(|| AppError::not_found("no OAuth apps eligible for refresh"))
    }

    /// Синхронный refresh токена аппки + UPSERT в `oauth_app_tokens`.
    /// `refresh_lock` сериализует issue-requests — мы не хотим одновременных
    /// `oauth/token` POST'ов с одного IP (попадаем в 30/1h/IP).
    pub async fn refresh_for_app(&self, app_id: Uuid) -> AppResult<()> {
        let _guard = self.refresh_lock.lock().await;
        let app = self
            .apps
            .get_by_id(&app_id.to_string())
            .await?
            .ok_or_else(|| AppError::not_found("OAuth app not found"))?;
        if !app.active {
            return Err(AppError::not_found("OAuth app inactive"));
        }

        match self
            .sc
            .exchange_client_credentials_for_token(&app.client_id, &app.client_secret)
            .await
        {
            Ok(token) => {
                let expires_at = Utc::now() + chrono::Duration::seconds(token.expires_in.max(0));
                let scope_opt = if token.scope.is_empty() {
                    None
                } else {
                    Some(token.scope)
                };
                sqlx::query(
                    "INSERT INTO oauth_app_tokens \
                        (oauth_app_id, access_token, scope, expires_at, \
                         refreshed_at, refresh_attempts, last_refresh_error) \
                     VALUES ($1, $2, $3, $4, now(), 0, NULL) \
                     ON CONFLICT (oauth_app_id) DO UPDATE SET \
                         access_token = EXCLUDED.access_token, \
                         scope = EXCLUDED.scope, \
                         expires_at = EXCLUDED.expires_at, \
                         refreshed_at = now(), \
                         refresh_attempts = 0, \
                         last_refresh_error = NULL",
                )
                .bind(app_id)
                .bind(&token.access_token)
                .bind(scope_opt)
                .bind(expires_at)
                .execute(&self.pg)
                .await?;
                info!(app = %app.name, "client_credentials token refreshed");
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                sqlx::query_file!(
                    "queries/oauth_apps/token_service/record_refresh_error.sql",
                    app_id,
                    msg
                )
                .execute(&self.pg)
                .await?;
                warn!(app = %app.name, error = %msg, "client_credentials refresh failed");
                Err(e)
            }
        }
    }

    pub fn spawn_refresh_loop(self: Arc<Self>, shutdown: CancellationToken) {
        tokio::spawn(async move {
            if let Err(e) = self.tick_once().await {
                warn!(error = %e, "initial oauth_app_tokens warmup failed");
            }
            let _ = self.reload_snapshot().await;

            let mut ticker = tokio::time::interval(REFRESH_TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = self.tick_once().await {
                            warn!(error = %e, "oauth_app_tokens refresh tick failed");
                        }
                        if let Err(e) = self.reload_snapshot().await {
                            warn!(error = %e, "oauth_app_tokens snapshot reload failed");
                        }
                    }
                }
            }
        });
    }

    async fn tick_once(&self) -> AppResult<()> {
        let active_ids: Vec<Uuid> = self
            .apps
            .find_all()
            .await?
            .into_iter()
            .filter(|a| a.active)
            .map(|a| a.id)
            .collect();
        if active_ids.is_empty() {
            return Ok(());
        }
        let lead = REFRESH_LEAD.num_seconds();
        let due: Vec<Uuid> = sqlx::query_file_scalar!(
            "queries/oauth_apps/token_service/due_apps.sql",
            &active_ids,
            lead as f64,
            MAX_REFRESH_ATTEMPTS,
            RETRY_COOLDOWN_SECS as f64
        )
        .fetch_all(&self.pg)
        .await?;

        for (i, app_id) in due.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(REFRESH_GAP).await;
            }
            if let Err(e) = self.refresh_for_app(*app_id).await {
                debug!(app = %app_id, error = %e, "refresh in tick failed (will retry)");
            }
        }
        Ok(())
    }
}

fn shuffled(src: &Arc<Vec<String>>) -> Vec<String> {
    let mut out = src.as_ref().clone();
    out.shuffle(&mut rand::thread_rng());
    out
}
