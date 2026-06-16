//! Internal pipeline endpoint — только для backend'а.
//!
//! `POST /internal/transcode-upload/:track_urn` — Bearer=INTERNAL_TOKEN.
//! Возвращает `202 Accepted` сразу после auth+HEAD-проверки. Если файл уже
//! в storage — сразу `200 {cached:true}`. Иначе download + storage-upload
//! идут фоновой tokio-таской; завершение приходит к backend'у NATS-евентом
//! `storage.track_uploaded`, который шлёт сам storage по результату S3 PUT.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use bytes::Bytes;
use reqwest::Client;
use serde::Serialize;
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::stream::storage::{
    is_canonical_track_urn, lookup_expected_duration_ms, upload_to_storage, StorageClient,
    UploadError,
};
use crate::AppState;

static WVD_CURSOR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `GET /internal/wvd` — serve a `.wvd` device to a relay client for relay-side
/// Widevine decrypt. Gated by `x-wvd-token` == `SC_EDGE_WVD_TOKEN`; devices come
/// from `SC_EDGE_WVD_DIR` (a folder separate from `SC_DECRYPT_DEVICE`). Disabled
/// (404) unless both env are set.
pub async fn serve_wvd(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Bytes), (StatusCode, String)> {
    let (Some(dir), Some(token)) = (
        state.config.edge_wvd_dir.as_deref(),
        state.config.edge_wvd_token.as_deref(),
    ) else {
        return Err((StatusCode::NOT_FOUND, "wvd serving disabled".into()));
    };

    let provided = headers
        .get("x-wvd-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !bool::from(provided.as_bytes().ct_eq(token.as_bytes())) {
        return Err((StatusCode::UNAUTHORIZED, "bad wvd token".into()));
    }

    let bytes = pick_wvd(dir)
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e))?;
    let mut h = HeaderMap::new();
    h.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/octet-stream"),
    );
    Ok((h, bytes))
}

async fn pick_wvd(dir: &str) -> Result<Bytes, String> {
    let mut rd = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| format!("read dir: {e}"))?;
    let mut files = Vec::new();
    while let Some(e) = rd
        .next_entry()
        .await
        .map_err(|e| format!("dir entry: {e}"))?
    {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "wvd") {
            files.push(p);
        }
    }
    if files.is_empty() {
        return Err("no .wvd in SC_EDGE_WVD_DIR".into());
    }
    files.sort();
    let idx = WVD_CURSOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % files.len();
    let data = tokio::fs::read(&files[idx])
        .await
        .map_err(|e| format!("read wvd: {e}"))?;
    Ok(Bytes::from(data))
}

/// Глобальный лимит одновременно качающихся треков. Backend дедупит триггеры
/// 16-широким семафором, но streaming могут долбить и ручные ретраи /
/// несколько backend'ов — оставляем свой потолок.
static FETCH_SEM: once_cell::sync::Lazy<Arc<Semaphore>> =
    once_cell::sync::Lazy::new(|| Arc::new(Semaphore::new(8)));

#[derive(Serialize)]
pub struct TranscodeUploadResponse {
    pub status: &'static str,
    pub url: String,
    pub cached: bool,
}

pub async fn transcode_upload(
    State(state): State<AppState>,
    Path(track_urn): Path<String>,
    headers: HeaderMap,
) -> Result<Json<TranscodeUploadResponse>, (StatusCode, String)> {
    check_auth(&headers, &state.config.internal_token)?;

    if !is_canonical_track_urn(&track_urn) {
        return Err((
            StatusCode::BAD_REQUEST,
            "track_urn must be a canonical soundcloud:tracks:<id> URN".into(),
        ));
    }

    if state.config.storage_url.is_empty() || state.config.storage_token.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "storage not configured".into(),
        ));
    }

    let filename = StorageClient::track_filename(&track_urn);
    let storage_base = state.config.storage_url.trim_end_matches('/');
    let key = format!("{filename}.m4a");
    let head_url = format!("{storage_base}/{key}");
    let redirect_url = format!("{storage_base}/redirect/{key}");

    if head_ok(&state.http_client, &head_url).await {
        return Ok(Json(TranscodeUploadResponse {
            status: "cached",
            url: redirect_url,
            cached: true,
        }));
    }

    let task_state = state.clone();
    let task_urn = track_urn.clone();
    let task_head = head_url.clone();
    let task_filename = filename.clone();
    tokio::spawn(async move {
        let _permit = match FETCH_SEM.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return,
        };
        if head_ok(&task_state.http_client, &task_head).await {
            return;
        }
        let (data, quality) = match fetch_track(&task_state, &task_urn).await {
            Some(d) => d,
            None => {
                warn!("[internal/transcode-upload] {task_urn} no stream available");
                return;
            }
        };
        let upload_base = task_state.config.storage_upload_url.trim_end_matches('/');
        let expected_ms = lookup_expected_duration_ms(&task_state.pg, &task_urn).await;
        match upload_to_storage(
            &task_state.http_client,
            upload_base,
            &task_state.config.storage_token,
            &task_filename,
            &data,
            quality,
            expected_ms,
        )
        .await
        {
            Ok(()) => info!(
                "[internal/transcode-upload] {task_urn} uploaded {:.1}MB",
                data.len() as f64 / 1024.0 / 1024.0
            ),
            Err(UploadError::Rejected { status, body }) => {
                info!("[internal/transcode-upload] {task_urn} rejected ({status}): {body}");
            }
            Err(e) => warn!("[internal/transcode-upload] upload {task_urn} failed: {e}"),
        }
    });

    Ok(Json(TranscodeUploadResponse {
        status: "accepted",
        url: redirect_url,
        cached: false,
    }))
}

fn check_auth(headers: &HeaderMap, expected: &str) -> Result<(), (StatusCode, String)> {
    if expected.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "internal endpoint disabled".into(),
        ));
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing token".into()))?;
    if token.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() != 1 {
        return Err((StatusCode::FORBIDDEN, "invalid token".into()));
    }
    Ok(())
}

async fn head_ok(client: &Client, url: &str) -> bool {
    match client
        .head(url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Cascade: cookies(HQ) → cookies(SQ) → anon. Returns the bytes plus the quality
/// (`hq`/`sq`) actually obtained, so storage records `storage_quality` correctly
/// instead of defaulting everything to `sq`.
/// OAuth здесь не используется (нет сессии) — только анонимные/cookies пути.
async fn fetch_track(state: &AppState, track_urn: &str) -> Option<(Bytes, &'static str)> {
    let tag = "[internal/fetch]";

    if let Some(cookies) = state.cookies.as_ref() {
        if let Ok(Some(result)) = cookies.get_stream(track_urn, true).await {
            info!("{tag} {track_urn} → cookies/hq");
            return Some((result.data, "hq"));
        }
        if let Ok(Some(result)) = cookies.get_stream(track_urn, false).await {
            info!("{tag} {track_urn} → cookies/sq");
            return Some((result.data, "sq"));
        }
    }

    if let Ok(Some(result)) = state.anon.get_stream(track_urn).await {
        info!("{tag} {track_urn} → anon");
        return Some((result.data, "sq"));
    }

    warn!("{tag} {track_urn} → no stream available");
    None
}
