//! Единая точка выдачи SC-токенов потребителям бекенда.
//!
//! Разделение по контексту вызова:
//! * [`TokenKind::User`] — `/me/*`, like/follow/upload, изменения, private content.
//!   Только токен сессии; нет fallback'а на public-пул (operations require user identity).
//! * [`TokenKind::UserFirst`] — public-операция, инициированная конкретным юзером
//!   (поиск из UI, resolve из UI). User-token первым (квотируется на юзера,
//!   лучше персонализация); далее — весь public-пул в перемешанном порядке.
//! * [`TokenKind::PublicPool`] — фоновые задачи: discovery cron'ы, cold-refresh
//!   без user-контекста, enrich SC-lookups. Только пул oauth_app_tokens
//!   (весь, перемешанный).
//!
//! `try_each` инкапсулирует ротацию: каллер передаёт closure, она исполняется
//! по очереди для каждого токена из chain'а, пока один не вернёт `Ok` или не
//! упадёт с ошибкой, для которой ротация бессмысленна (см. [`should_rotate`]).

use std::sync::Arc;

use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::modules::auth::AuthService;
use crate::modules::oauth_apps::OAuthAppTokenService;
use crate::sc;

#[derive(Debug, Clone, Copy)]
pub enum TokenKind {
    User(Uuid),
    UserFirst(Uuid),
    PublicPool,
}

pub struct TokenProvider {
    auth: Arc<AuthService>,
    app_tokens: Arc<OAuthAppTokenService>,
}

impl TokenProvider {
    pub fn new(auth: Arc<AuthService>, app_tokens: Arc<OAuthAppTokenService>) -> Arc<Self> {
        Arc::new(Self { auth, app_tokens })
    }

    /// Полный fan-out порядок: какие токены пробовать и в каком порядке.
    /// Для `UserFirst`/`PublicPool` public-токены перемешиваются на каждом
    /// вызове, чтобы не упирать одну аппку в SC rate limit.
    pub async fn chain(&self, kind: TokenKind) -> AppResult<Vec<String>> {
        match kind {
            TokenKind::User(session_id) => {
                Ok(vec![self.auth.get_valid_access_token(session_id).await?])
            }
            TokenKind::UserFirst(session_id) => {
                let mut out: Vec<String> = Vec::new();
                if let Ok(t) = self.auth.get_valid_access_token(session_id).await {
                    if !t.is_empty() {
                        out.push(t);
                    }
                }
                for t in self.app_tokens.snapshot().await.unwrap_or_default() {
                    if !out.contains(&t) {
                        out.push(t);
                    }
                }
                if out.is_empty() {
                    return Err(AppError::internal("no tokens available for UserFirst"));
                }
                Ok(out)
            }
            TokenKind::PublicPool => {
                let out = self.app_tokens.snapshot().await?;
                if out.is_empty() {
                    return Err(AppError::internal("public-token pool empty"));
                }
                Ok(out)
            }
        }
    }
}

/// Прокачать closure через готовый chain — на каждой итерации новый токен,
/// до первого успеха. Ротация только на ошибках, где смена токена может
/// помочь (auth/rate-limit/ban) — для прочих сразу пробрасываем.
/// Caller'ы вычисляют chain один раз через [`TokenProvider::chain`] и далее
/// гоняют через эту функцию (например, в paginated-fetch'ах per-chunk —
/// иначе get_valid_access_token бьёт БД на каждый chunk).
pub async fn try_with_chain<F, Fut, T>(chain: &[String], op: F) -> AppResult<T>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = AppResult<T>>,
{
    let mut last_err: Option<AppError> = None;
    for tok in chain {
        match op(tok.clone()).await {
            Ok(v) => return Ok(v),
            Err(e) if should_rotate(&e) => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::internal("no tokens worked")))
}

/// Стоит ли пробовать следующий токен из chain'а после такой ошибки.
/// `401` — токен невалиден; `429`/`is_ban_error` — текущий identity заблочен,
/// у другого может пройти.
pub fn should_rotate(err: &AppError) -> bool {
    matches!(err, AppError::ScApi { status: 401, .. })
        || sc::is_rate_limited(err)
        || sc::is_ban_error(err)
}
