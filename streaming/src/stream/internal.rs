//! Internal pipeline endpoint — только для backend'а.
//!
//! `POST /internal/transcode-upload/:track_urn` — Bearer=INTERNAL_TOKEN.
//!
//! 1. Проверяет, что файл уже есть в storage (HEAD) — возвращает стабильный redirect-URL.
//! 2. Иначе: качает трек (cookies HQ → oauth HQ → oauth SQ → anon), НЕ зависит от premium-only.
//! 3. Заливает в storage через multipart /upload (storage транскодит в HQ+SQ Opus).
//! 4. Возвращает `{ url }` вида `{storage}/redirect/hq/{file}.ogg`. В S3-режиме storage
//!    на каждый хит пересчитывает свежий presigned и делает 307 → worker следует по нему.
//!    В local-режиме storage отдаёт байты сам.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use bytes::Bytes;
use reqwest::Client;
use serde::Serialize;
use tracing::{info, warn};

use crate::stream::storage::StorageClient;
use crate::AppState;

#[derive(Serialize)]
pub struct TranscodeUploadResponse {
    pub url: String,
    pub size_bytes: usize,
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
    let hq_key = format!("hq/{filename}.ogg");
    let hq_head_url = format!("{storage_base}/{hq_key}");
    let redirect_url = format!("{storage_base}/redirect/{hq_key}");

    // 1. Уже лежит в storage → сразу отдаём стабильный redirect-URL.
    if head_ok(&state.http_client, &hq_head_url).await {
        info!("[internal/transcode-upload] {track_urn} already in storage");
        return Ok(Json(TranscodeUploadResponse {
            url: redirect_url,
            size_bytes: 0,
            cached: true,
        }));
    }

    // 2. Качаем: HQ cookies → HQ oauth → SQ oauth → SQ cookies → anon.
    //    Если упали, но файл параллельно появился в storage — не считаем ошибкой.
    let data = match fetch_track(&state, &track_urn).await {
        Some(d) => d,
        None => {
            if head_ok(&state.http_client, &hq_head_url).await {
                info!("[internal/transcode-upload] {track_urn} appeared in storage after fetch fail");
                return Ok(Json(TranscodeUploadResponse {
                    url: redirect_url,
                    size_bytes: 0,
                    cached: true,
                }));
            }
            return Err((StatusCode::BAD_GATEWAY, "no stream available".into()));
        }
    };

    // 3. Заливаем в storage (multipart). Storage транскодит в hq+sq Opus.
    //    Если upload вернул ошибку, но файл уже лежит (гонка/повторный запрос) — ок.
    let upload_base = state.config.storage_upload_url.trim_end_matches('/');
    if let Err(e) = upload_to_storage(
        &state.http_client,
        upload_base,
        &state.config.storage_token,
        &filename,
        &data,
    )
    .await
    {
        if head_ok(&state.http_client, &hq_head_url).await {
            info!(
                "[internal/transcode-upload] {track_urn} upload failed ({e}) but file present"
            );
            return Ok(Json(TranscodeUploadResponse {
                url: redirect_url,
                size_bytes: 0,
                cached: true,
            }));
        }
        warn!("[internal/transcode-upload] upload {track_urn} failed: {e}");
        return Err((StatusCode::BAD_GATEWAY, format!("storage upload: {e}")));
    }

    info!(
        "[internal/transcode-upload] {track_urn} uploaded {:.1}MB",
        data.len() as f64 / 1024.0 / 1024.0
    );

    Ok(Json(TranscodeUploadResponse {
        url: redirect_url,
        size_bytes: data.len(),
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
