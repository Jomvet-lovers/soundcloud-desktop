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

    // 2. OAuth stream
    if let Some(result) = super::oauth::try_oauth_stream(
        &state.http_client,
        &state.config.sc_proxy_url,
        &session.access_token,
        &track_urn,
        secret_token,
    )
    .await
    {
        info!("[stream] {track_urn} → oauth");
        return respond_with_data(&state, &track_urn, result.data, result.content_type);
    }

    // 3. Anon stream
    match state.anon.get_stream(&track_urn).await {
        Ok(Some(result)) => {
            info!("[stream] {track_urn} → anon");
            return respond_with_data(&state, &track_urn, result.data, result.content_type);
        }
        Ok(None) => {}
        Err(e) => warn!("[stream] anon failed for {track_urn}: {e}"),
    }

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

    // Build user URN format: soundcloud:users:12345
    let user_urn_full = if user_urn.contains(':') {
        user_urn.to_string()
    } else {
        format!("soundcloud:users:{user_urn}")
    };

    let is_premium = state
        .sqlite
        .is_premium(&user_urn_full)
        .map_err(|e| AppError::Internal(format!("sqlite: {e}")))?;

    if !is_premium {
        return Err(AppError::Forbidden);
    }

    // 1. Try CDN
    if let Some(cdn_url) = state.cdn.try_serve(&track_urn, hq).await {
        info!("[stream/premium] {track_urn} → CDN redirect");
        return Ok(Redirect::temporary(&cdn_url).into_response());
    }

    if hq {
        // HQ mode: cookies → OAuth → anon
        if let Some(ref cookies_client) = state.cookies {
            match cookies_client.get_stream(&track_urn).await {
                Ok(Some(result)) => {
                    info!("[stream/premium] {track_urn} → cookies {}", result.quality);
                    return respond_with_data(&state, &track_urn, result.data, result.content_type);
                }
                Ok(None) => {}
                Err(e) => warn!("[stream/premium] cookies failed: {e}"),
            }
        }

        if let Some(result) = super::oauth::try_oauth_stream(
            &state.http_client,
            &state.config.sc_proxy_url,
            &session.access_token,
            &track_urn,
            secret_token,
        )
        .await
        {
            info!("[stream/premium] {track_urn} → oauth");
            return respond_with_data(&state, &track_urn, result.data, result.content_type);
        }

        match state.anon.get_stream(&track_urn).await {
            Ok(Some(result)) => {
                info!("[stream/premium] {track_urn} �� anon");
                return respond_with_data(&state, &track_urn, result.data, result.content_type);
            }
            Ok(None) => {}
            Err(e) => warn!("[stream/premium] anon failed: {e}"),
        }
    } else {
        // Non-HQ: OAuth → anon → cookies
        if let Some(result) = super::oauth::try_oauth_stream(
            &state.http_client,
            &state.config.sc_proxy_url,
            &session.access_token,
            &track_urn,
            secret_token,
        )
        .await
        {
            info!("[stream/premium] {track_urn} → oauth");
            return respond_with_data(&state, &track_urn, result.data, result.content_type);
        }

        match state.anon.get_stream(&track_urn).await {
            Ok(Some(result)) => {
                info!("[stream/premium] {track_urn} → anon");
                return respond_with_data(&state, &track_urn, result.data, result.content_type);
            }
            Ok(None) => {}
            Err(e) => warn!("[stream/premium] anon failed: {e}"),
        }

        if let Some(ref cookies_client) = state.cookies {
            match cookies_client.get_stream(&track_urn).await {
                Ok(Some(result)) => {
                    info!("[stream/premium] {track_urn} → cookies {}", result.quality);
                    return respond_with_data(&state, &track_urn, result.data, result.content_type);
                }
                Ok(None) => {}
                Err(e) => warn!("[stream/premium] cookies failed: {e}"),
            }
        }
    }

    Err(AppError::NoStream)
}

fn extract_session_id(headers: &HeaderMap, query: &StreamQuery) -> Result<String, AppError> {
    // Try x-session-id header first, then query param
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
    // Upload to CDN in background
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
