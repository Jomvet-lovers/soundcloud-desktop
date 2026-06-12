use axum::extract::{FromRequestParts, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::common::session::SessionCtx;
use crate::error::AppError;
use crate::state::AppState;

/// premium_reserve-гейт: всё кроме вайтлиста и OPTIONS требует премиум-сессию
/// (нет сессии → 401, не-премиум → 403). Когда режим выключен — passthrough.
pub async fn premium_gate(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.config.premium_reserve {
        return next.run(req).await;
    }
    if req.method() == Method::OPTIONS || is_open_path(req.uri().path()) {
        return next.run(req).await;
    }

    let (mut parts, body) = req.into_parts();
    let ctx = match SessionCtx::from_request_parts(&mut parts, &state).await {
        Ok(c) => c,
        Err(e) => return e.into_response(),
    };
    match state.subscriptions.is_premium(&ctx.sc_user_id).await {
        Ok(true) => {}
        Ok(false) => return AppError::forbidden("Star subscription required").into_response(),
        Err(e) => return e.into_response(),
    }

    next.run(Request::from_parts(parts, body)).await
}

/// Открыто без премиум-сессии: health, весь OAuth/login-флоу (только по нему
/// юзер и получает сессию), /me/subscription (bootstrap-сигнал премиума —
/// хэндлер сам требует сессию, не-премиуму отдаёт {"premium":false}), ACME HTTP-01.
fn is_open_path(path: &str) -> bool {
    path == "/health"
        || path == "/me/subscription"
        || path.starts_with("/auth/")
        || path.starts_with("/.well-known/")
}
