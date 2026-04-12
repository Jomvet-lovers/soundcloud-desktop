use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use bytes::Bytes;
use tracing::{info, warn};

use crate::error::AppError;
use crate::AppState;

#[derive(serde::Deserialize)]
pub struct StreamQuery {
    pub hq: Option<String>,
    pub session_id: Option<String>,
    pub secret_token: Option<String>,
}

/// GET /stream/:track_urn — normal stream (OAuth → anon)
pub async fn stream_normal(
    State(state): State<AppState>,
    Path(track_urn): Path<String>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> Result<Response, AppError> {
    let session_id = extract_session_id(&headers, &query)?;
    let session = state
        .pg
        .get_session(&session_id)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let hq = query.hq.as_deref() == Some("true");
    let secret_token = query.secret_token.as_deref();

    // 1. Try CDN
    if let Some(cdn_url) = state.cdn.try_serve(&track_urn, hq).await {
        info!("[stream] {track_urn} → CDN redirect");
        return Ok(Redirect::temporary(&cdn_url).into_response());
    }

    // 2. OAuth(all) → anon
    if let Some(resp) = try_oauth(&state, &session.access_token, &track_urn, secret_token, false).await {
        info!("[stream] {track_urn} → oauth");
        return respond_with_data(&state, &track_urn, resp.0, resp.1);
    }

    if let Some(resp) = try_anon(&state, &track_urn, "[stream]").await {
        info!("[stream] {track_urn} → anon");
        return respond_with_data(&state, &track_urn, resp.0, resp.1);
    }

    warn!("[stream] {track_urn} → no stream available");
    Err(AppError::NoStream)
}

/// GET /stream/:track_urn/premium — premium stream (subscription check, cookies HQ → OAuth → anon)
pub async fn stream_premium(
    State(state): State<AppState>,
    Path(track_urn): Path<String>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> Result<Response, AppError> {
    let session_id = extract_session_id(&headers, &query)?;
    let session = state
        .pg
        .get_session(&session_id)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let hq = query.hq.as_deref() == Some("true");
    let secret_token = query.secret_token.as_deref();

    // Check subscription
    let user_urn = session
        .soundcloud_user_id
        .as_deref()
        .ok_or(AppError::Forbidden)?;

    let user_urn_full = if user_urn.contains(':') {
        user_urn.to_string()
    } else {
        format!("soundcloud:users:{user_urn}")
    };

    let is_premium = state.pg.is_premium(&user_urn_full).await?;

    if !is_premium {
        return Err(AppError::Forbidden);
    }

    // 1. Try CDN
    if let Some(cdn_url) = state.cdn.try_serve(&track_urn, hq).await {
        info!("[stream/premium] {track_urn} → CDN redirect");
        return Ok(Redirect::temporary(&cdn_url).into_response());
    }

    let tag = "[stream/premium]";

    if hq {
        // HQ cascade: cookies(HQ) → oauth(HQ) → oauth(all) → cookies(all) → anon
        if let Some(resp) = try_cookies(&state, &track_urn, tag, true).await {
            info!("{tag} {track_urn} → cookies/hq");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }

        if let Some(resp) = try_oauth(&state, &session.access_token, &track_urn, secret_token, true).await {
            info!("{tag} {track_urn} → oauth/hq");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }

        if let Some(resp) = try_oauth(&state, &session.access_token, &track_urn, secret_token, false).await {
            info!("{tag} {track_urn} → oauth/sq");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }

        if let Some(resp) = try_cookies(&state, &track_urn, tag, false).await {
            info!("{tag} {track_urn} → cookies/sq");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }

        if let Some(resp) = try_anon(&state, &track_urn, tag).await {
            info!("{tag} {track_urn} → anon");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }
    } else {
        // SQ cascade: oauth(all) → anon → cookies(all)
        if let Some(resp) = try_oauth(&state, &session.access_token, &track_urn, secret_token, false).await {
            info!("{tag} {track_urn} → oauth");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }

        if let Some(resp) = try_anon(&state, &track_urn, tag).await {
            info!("{tag} {track_urn} → anon");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }

        if let Some(resp) = try_cookies(&state, &track_urn, tag, false).await {
            info!("{tag} {track_urn} → cookies");
            return respond_with_data(&state, &track_urn, resp.0, resp.1);
        }
    }

    warn!("{tag} {track_urn} → no stream available");
    Err(AppError::NoStream)
}

// ── Fallback helpers ──────────────────────────────────────────

async fn try_oauth(
    state: &AppState,
    access_token: &str,
    track_urn: &str,
    secret_token: Option<&str>,
    hq_only: bool,
) -> Option<(Bytes, &'static str)> {
    let result = super::oauth::try_oauth_stream(
        &state.http_client,
        &state.config.sc_proxy_url,
        state.config.sc_proxy_fallback,
        access_token,
        track_urn,
        secret_token,
        hq_only,
    )
    .await?;
    Some((result.data, result.content_type))
}

async fn try_cookies(
    state: &AppState,
    track_urn: &str,
    tag: &str,
    hq_only: bool,
) -> Option<(Bytes, &'static str)> {
    let cookies_client = state.cookies.as_ref()?;
    match cookies_client.get_stream(track_urn, hq_only).await {
        Ok(Some(result)) => Some((result.data, result.content_type)),
        Ok(None) => {
            warn!("{tag} {track_urn} cookies returned nothing");
            None
        }
        Err(e) => {
            warn!("{tag} {track_urn} cookies failed: {e}");
            None
        }
    }
}

async fn try_anon(state: &AppState, track_urn: &str, tag: &str) -> Option<(Bytes, &'static str)> {
    match state.anon.get_stream(track_urn).await {
        Ok(Some(result)) => Some((result.data, result.content_type)),
        Ok(None) => {
            warn!("{tag} {track_urn} anon returned nothing");
            None
        }
        Err(e) => {
            warn!("{tag} {track_urn} anon failed: {e}");
            None
        }
    }
}

// ── Shared ────────────────────────────────────────────────────

fn extract_session_id(headers: &HeaderMap, query: &StreamQuery) -> Result<String, AppError> {
    if let Some(val) = headers.get("x-session-id") {
        return val
            .to_str()
            .map(|s| s.to_string())
            .map_err(|_| AppError::Unauthorized);
    }
    query.session_id.clone().ok_or(AppError::Unauthorized)
}

fn respond_with_data(
    state: &AppState,
    track_urn: &str,
    data: Bytes,
    content_type: &'static str,
) -> Result<Response, AppError> {
    if data.len() > 8192 {
        state
            .cdn
            .upload_in_background(track_urn.to_string(), data.clone());
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("content-length", data.len().to_string())
        .body(Body::from(data))
        .unwrap())
}
