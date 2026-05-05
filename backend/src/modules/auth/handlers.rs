use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tracing::error;

use crate::common::session::RawSessionIdHeader;
use crate::common::uuid::parse_uuid;
use crate::error::{AppError, AppResult};
use crate::modules::auth::callback_page::{render as render_callback_page, CallbackPageParams};
use crate::modules::auth::dto::*;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/login/status", get(login_status))
        .route("/auth/callback", get(callback))
        .route("/auth/session", get(session))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/auth/link/create", post(link_create))
        .route("/auth/link/claim", post(link_claim))
        .route("/auth/link/status", get(link_status))
}

#[tracing::instrument(skip_all)]
async fn login(
    State(state): State<AppState>,
    RawSessionIdHeader(raw): RawSessionIdHeader,
) -> AppResult<Json<LoginResponse>> {
    let existing = raw.as_deref().and_then(parse_uuid);
    let result = state.auth.initiate_login(existing).await?;
    Ok(Json(LoginResponse {
        url: result.url,
        login_request_id: result.login_request_id,
    }))
}

#[tracing::instrument(skip(state))]
async fn login_status(
    State(state): State<AppState>,
    Query(q): Query<LoginStatusQuery>,
) -> AppResult<Json<Value>> {
    let Some(id) = parse_uuid(&q.id) else {
        return Ok(Json(json!({
            "status": "expired",
            "error": "Invalid login request id",
        })));
    };
    let r = state.auth.get_login_request_status(id).await?;
    Ok(Json(serde_json::to_value(&r).map_err(|e| {
        AppError::internal(format!("encode login status: {e}"))
    })?))
}

#[tracing::instrument(skip(state))]
async fn callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let html = match state.auth.handle_callback(&q.code, &q.state).await {
        Ok(result) => {
            let login_id = result.login_request_id.map(|u| u.to_string());
            let params = CallbackPageParams {
                login_request_id: login_id.as_deref(),
                initial_status: result.initial_status.as_str(),
                username: result.username.as_deref(),
                error: result.error.as_deref(),
            };
            render_callback_page(&params)
        }
        Err(err) => {
            error!(error = %err, "Unhandled error in /auth/callback");
            let params = CallbackPageParams {
                login_request_id: None,
                initial_status: "failed",
                username: None,
                error: Some("Authentication failed due to a server error. Please try again."),
            };
            render_callback_page(&params)
        }
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

#[tracing::instrument(skip_all)]
async fn session(
    State(state): State<AppState>,
    RawSessionIdHeader(raw): RawSessionIdHeader,
) -> AppResult<Json<SessionResponse>> {
    let Some(session_id) = raw.as_deref().and_then(parse_uuid) else {
        return Ok(Json(SessionResponse {
            authenticated: false,
            session_id: None,
            username: None,
            soundcloud_user_id: None,
            expires_at: None,
        }));
    };

    let session = state.auth.get_session(session_id).await?;
    match session {
        Some(s) if !s.access_token.is_empty() => Ok(Json(SessionResponse {
            authenticated: true,
            session_id: Some(s.id),
            username: s.username,
            soundcloud_user_id: s.soundcloud_user_id,
            expires_at: Some(s.expires_at),
        })),
        _ => Ok(Json(SessionResponse {
            authenticated: false,
            session_id: None,
            username: None,
            soundcloud_user_id: None,
            expires_at: None,
        })),
    }
}

#[tracing::instrument(skip_all)]
async fn refresh(
    State(state): State<AppState>,
    RawSessionIdHeader(raw): RawSessionIdHeader,
) -> AppResult<Json<RefreshResponse>> {
    let session_id = raw
        .as_deref()
        .and_then(parse_uuid)
        .ok_or_else(|| AppError::unauthorized("Malformed session id"))?;
    let session = state.auth.refresh_session(session_id).await?;
    Ok(Json(RefreshResponse {
        session_id: session.id,
        expires_at: session.expires_at,
    }))
}

#[tracing::instrument(skip_all)]
async fn logout(
    State(state): State<AppState>,
    RawSessionIdHeader(raw): RawSessionIdHeader,
) -> AppResult<Json<LogoutResponse>> {
    if let Some(session_id) = raw.as_deref().and_then(parse_uuid) {
        state.auth.logout(session_id).await?;
    }
    Ok(Json(LogoutResponse { success: true }))
}

#[tracing::instrument(skip_all)]
async fn link_create(
    State(state): State<AppState>,
    RawSessionIdHeader(raw): RawSessionIdHeader,
    Json(body): Json<CreateLinkRequest>,
) -> AppResult<Json<CreateLinkResponse>> {
    let source = raw.as_deref().and_then(parse_uuid);
    let result = state.link.create(&body.mode, source).await?;
    Ok(Json(CreateLinkResponse {
        link_request_id: result.link_request_id,
        claim_token: result.claim_token,
        expires_at: result.expires_at,
    }))
}

#[tracing::instrument(skip_all)]
async fn link_claim(
    State(state): State<AppState>,
    RawSessionIdHeader(raw): RawSessionIdHeader,
    Json(body): Json<ClaimLinkRequest>,
) -> AppResult<Json<ClaimLinkResponse>> {
    let source = raw.as_deref().and_then(parse_uuid);
    let r = state.link.claim(&body.claim_token, source).await?;
    Ok(Json(ClaimLinkResponse {
        session_id: r.session_id,
        mode: r.mode,
    }))
}

#[tracing::instrument(skip(state))]
async fn link_status(
    State(state): State<AppState>,
    Query(q): Query<LinkStatusQuery>,
) -> AppResult<Json<LinkStatusResponse>> {
    let Some(id) = parse_uuid(&q.id) else {
        return Ok(Json(LinkStatusResponse {
            status: "expired".into(),
            mode: "pull".into(),
            session_id: None,
            error: Some("Unknown link request".into()),
        }));
    };
    let r = state.link.get_status(id).await?;
    Ok(Json(LinkStatusResponse {
        status: r.status,
        mode: r.mode,
        session_id: r.session_id,
        error: r.error,
    }))
}
