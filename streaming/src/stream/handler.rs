use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use bytes::Bytes;
use futures::StreamExt;
use tracing::{info, warn};

use crate::db::postgres::SessionInfo;
use crate::error::AppError;
use crate::AppState;

/// Верхняя граница на один запрос — не даём каскадам fallback'ов
/// удерживать клиентское подключение бесконечно.
/// `/stream` может долго гонять oauth ретраи + cookies + restricted, поэтому потолок выше.
/// `/download` отдаёт только метаданные, ему хватает минуты.
/// Оба чуть ниже клиентских read_timeout'ов, чтобы сервер успевал ответить первым.
pub(crate) const STREAM_DEADLINE: Duration = Duration::from_secs(120);
pub(crate) const DOWNLOAD_DEADLINE: Duration = Duration::from_secs(60);

#[derive(serde::Deserialize)]
pub struct StreamQuery {
    pub hq: Option<String>,
    pub session_id: Option<String>,
    pub secret_token: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct ResolveQuery {
    pub url: String,
}

pub async fn resolve_track(
    State(state): State<AppState>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    match state.anon.resolve_url(&query.url).await {
        Ok(track) => Ok(Json(track)),
        Err(error) => {
            warn!("[resolve] {} failed: {error}", query.url);
            Err(AppError::NotFound)
        }
    }
}

/// GET /stream/:track_urn — единый endpoint.
///
/// * `premium_only` хост и не-премиум юзер → 403.
/// * `hq=true` без премиум → 403 (HQ требует подписки).
/// * Дальше каскад по `hq`:
///   - HQ: oauth(hq) → cookies(hq) → restricted(hq) → oauth(sq) → anon → cookies(sq) → restricted(sq).
///   - SQ: oauth → anon → cookies (если премиум) → restricted(sq).
pub async fn stream(
    state: State<AppState>,
    track_urn: Path<String>,
    headers: HeaderMap,
    query: Query<StreamQuery>,
) -> Result<Response, AppError> {
    let urn_for_log = track_urn.0.clone();
    match tokio::time::timeout(STREAM_DEADLINE, stream_inner(state, track_urn, headers, query))
        .await
    {
        Ok(r) => r,
        Err(_) => {
            warn!("[stream] {urn_for_log} → deadline {STREAM_DEADLINE:?} exceeded");
            Err(AppError::NoStream)
        }
    }
}

async fn stream_inner(
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

    let is_premium = check_is_premium(&state, &session).await;
    let hq = query.hq.as_deref() == Some("true");
    let secret_token = query.secret_token.as_deref();

    if state.config.premium_only && !is_premium {
        return Err(AppError::Forbidden);
    }
    if hq && !is_premium {
        return Err(AppError::Forbidden);
    }

    let tag = "[stream]";

    // CDN first
    if let Some(cdn_url) = state.storage.try_serve(&track_urn).await {
        info!("{tag} {track_urn} → CDN redirect");
        return Ok(Redirect::temporary(&cdn_url).into_response());
    }

    let access = &session.access_token;

    if hq {
        if let Some(r) = try_oauth(&state, access, &track_urn, secret_token, true).await {
            info!("{tag} {track_urn} → oauth/hq");
            return respond_with_data(&state, &track_urn, r.0, r.1);
        }
        if let Some(r) = try_cookies(&state, &track_urn, tag, true).await {
            info!("{tag} {track_urn} → cookies/hq");
            return respond_with_data(&state, &track_urn, r.0, r.1);
        }
        if let Some(r) = try_restricted(&state, &track_urn, tag, true).await {
            info!("{tag} {track_urn} → restricted/hq");
            return Ok(r);
        }
        if let Some(r) = try_oauth(&state, access, &track_urn, secret_token, false).await {
            info!("{tag} {track_urn} → oauth/sq");
            return respond_with_data(&state, &track_urn, r.0, r.1);
        }
        if let Some(r) = try_anon(&state, &track_urn, tag).await {
            info!("{tag} {track_urn} → anon");
            return respond_with_data(&state, &track_urn, r.0, r.1);
        }
        if let Some(r) = try_cookies(&state, &track_urn, tag, false).await {
            info!("{tag} {track_urn} → cookies/sq");
            return respond_with_data(&state, &track_urn, r.0, r.1);
        }
    } else {
        if let Some(r) = try_oauth(&state, access, &track_urn, secret_token, false).await {
            info!("{tag} {track_urn} → oauth");
            return respond_with_data(&state, &track_urn, r.0, r.1);
        }
        if let Some(r) = try_anon(&state, &track_urn, tag).await {
            info!("{tag} {track_urn} → anon");
            return respond_with_data(&state, &track_urn, r.0, r.1);
        }
        if is_premium {
            if let Some(r) = try_cookies(&state, &track_urn, tag, false).await {
                info!("{tag} {track_urn} → cookies");
                return respond_with_data(&state, &track_urn, r.0, r.1);
            }
        }
    }

    if let Some(r) = try_restricted(&state, &track_urn, tag, false).await {
        info!("{tag} {track_urn} → restricted");
        return Ok(r);
    }

    warn!("{tag} {track_urn} → no stream available");
    Err(AppError::NoStream)
}

// ── Premium check ─────────────────────────────────────────────

pub(crate) async fn check_is_premium(state: &AppState, session: &SessionInfo) -> bool {
    let Some(user) = session.soundcloud_user_id.as_deref() else {
        return false;
    };
    let user_urn = if user.contains(':') {
        user.to_string()
    } else {
        format!("soundcloud:users:{user}")
    };
    state.pg.is_premium(&user_urn).await.unwrap_or(false)
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
        &state.pg,
        &state.config.sc_proxy_url,
        state.config.sc_proxy_fallback,
        state.config.sc_oauth_fallback_sessions,
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

async fn restricted_source(
    state: &AppState,
    track_urn: &str,
    tag: &str,
    hq_first: bool,
) -> Option<crate::stream::restricted::RestrictedSource> {
    match state.anon.resolve_restricted(track_urn, hq_first).await {
        Ok(Some(v)) => return Some(v),
        Ok(None) => {}
        Err(e) => warn!("{tag} {track_urn} restricted(anon) failed: {e}"),
    }
    let cookies = state.cookies.as_ref()?;
    match cookies.resolve_restricted(track_urn, hq_first).await {
        Ok(Some(v)) => Some(v),
        Ok(None) => None,
        Err(e) => {
            warn!("{tag} {track_urn} restricted(cookies) failed: {e}");
            None
        }
    }
}

async fn try_restricted(
    state: &AppState,
    track_urn: &str,
    tag: &str,
    hq_first: bool,
) -> Option<Response> {
    let engine = state.decryptor.as_ref()?;
    let src = restricted_source(state, track_urn, tag, hq_first).await?;
    let fetcher: std::sync::Arc<dyn decrypt::Fetcher> =
        std::sync::Arc::new(crate::stream::decrypt_fetch::ProxyFetcher {
            client: state.http_client.clone(),
            proxy_url: state.config.sc_proxy_url.clone(),
        });
    let stream = match engine
        .process_stream(&src.manifest, &src.token, fetcher)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!("{tag} {track_urn} restricted decode failed: {e}");
            return None;
        }
    };

    // Стримим клиенту чанками + копим, по завершении кэшируем в storage.
    let acc = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let acc_w = acc.clone();
    let storage = state.storage.clone();
    let urn = track_urn.to_string();
    let teed = stream
        .map(move |chunk| {
            if let Ok(b) = &chunk {
                acc_w.lock().unwrap().extend_from_slice(b);
            }
            chunk
        })
        .chain(futures::stream::once(async move {
            let data = std::mem::take(&mut *acc.lock().unwrap());
            if data.len() > 8192 {
                storage.upload_in_background(urn, Bytes::from(data));
            }
            Ok::<_, decrypt::Error>(Bytes::new())
        }));

    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", src.content_type)
            .body(Body::from_stream(teed))
            .unwrap(),
    )
}

// ── Shared ────────────────────────────────────────────────────

pub(crate) fn extract_session_id(
    headers: &HeaderMap,
    query: &StreamQuery,
) -> Result<String, AppError> {
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
            .storage
            .upload_in_background(track_urn.to_string(), data.clone());
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("content-length", data.len().to_string())
        .body(Body::from(data))
        .unwrap())
}
