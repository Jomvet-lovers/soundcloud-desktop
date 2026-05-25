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
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::stream::storage::StorageClient;
use crate::AppState;

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
        let data = match fetch_track(&task_state, &task_urn).await {
            Some(d) => d,
            None => {
                warn!("[internal/transcode-upload] {task_urn} no stream available");
                return;
            }
        };
        let upload_base = task_state.config.storage_upload_url.trim_end_matches('/');
        if let Err(e) = upload_to_storage(
            &task_state.http_client,
            upload_base,
            &task_state.config.storage_token,
            &task_filename,
            &data,
        )
        .await
        {
            warn!("[internal/transcode-upload] upload {task_urn} failed: {e}");
            return;
        }
        info!(
            "[internal/transcode-upload] {task_urn} uploaded {:.1}MB",
            data.len() as f64 / 1024.0 / 1024.0
        );
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
    if token != expected {
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

/// Cascade: cookies(HQ) → oauth(HQ) → oauth(SQ) → cookies(SQ) → anon.
/// OAuth здесь не используется (нет сессии) — только анонимные/cookies пути.
async fn fetch_track(state: &AppState, track_urn: &str) -> Option<Bytes> {
    let tag = "[internal/fetch]";

    if let Some(cookies) = state.cookies.as_ref() {
        if let Ok(Some(result)) = cookies.get_stream(track_urn, true).await {
            info!("{tag} {track_urn} → cookies/hq");
            return Some(result.data);
        }
        if let Ok(Some(result)) = cookies.get_stream(track_urn, false).await {
            info!("{tag} {track_urn} → cookies/sq");
            return Some(result.data);
        }
    }

    if let Ok(Some(result)) = state.anon.get_stream(track_urn).await {
        info!("{tag} {track_urn} → anon");
        return Some(result.data);
    }

    warn!("{tag} {track_urn} → no stream available");
    None
}

async fn upload_to_storage(
    client: &Client,
    base_url: &str,
    auth_token: &str,
    filename: &str,
    data: &Bytes,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file_part = reqwest::multipart::Part::bytes(data.to_vec())
        .file_name("audio")
        .mime_str("audio/mpeg")?;

    let form = reqwest::multipart::Form::new()
        .text("filename", filename.to_string())
        .part("file", file_part);

    client
        .post(format!("{base_url}/upload"))
        .header("Authorization", format!("Bearer {auth_token}"))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}
