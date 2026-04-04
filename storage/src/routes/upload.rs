use std::sync::Arc;

use axum::extract::{Multipart, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use crate::AppState;

#[derive(serde::Serialize)]
pub struct UploadResponse {
    pub filename: String,
    pub hq_path: String,
    pub sq_path: String,
    pub duration_secs: f64,
}

pub async fn upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Auth
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing token".into()))?;

    if token != state.config.admin_token {
        return Err((StatusCode::FORBIDDEN, "invalid token".into()));
    }

    // Parse multipart: expect "file" field and optional "filename" field
    let mut filename: Option<String> = None;
    let mut tmp_file_path: Option<std::path::PathBuf> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("multipart error: {e}")))?
    {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "filename" => {
                filename = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| (StatusCode::BAD_REQUEST, format!("read filename: {e}")))?,
                );
            }
            "file" => {
                // Stream to /tmp — don't buffer entire file in RAM
                let id = uuid::Uuid::new_v4();
                let tmp_path = std::path::PathBuf::from(&state.config.tmp_path)
                    .join(format!("{id}.input"));

                let mut file = tokio::fs::File::create(&tmp_path)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("create tmp: {e}")))?;

                // Stream chunks to disk
                let mut stream = field;
                let mut total: u64 = 0;
                loop {
                    match stream.chunk().await {
                        Ok(Some(chunk)) => {
                            total += chunk.len() as u64;
                            file.write_all(&chunk).await.map_err(|e| {
                                (StatusCode::INTERNAL_SERVER_ERROR, format!("write tmp: {e}"))
                            })?;
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&tmp_path).await;
                            return Err((StatusCode::BAD_REQUEST, format!("read chunk: {e}")));
                        }
                    }
                }

                file.flush().await.map_err(|e| {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("flush: {e}"))
                })?;
                drop(file);

                if total == 0 {
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                    return Err((StatusCode::BAD_REQUEST, "empty file".into()));
                }

                info!("[upload] received {:.1}MB", total as f64 / 1024.0 / 1024.0);
                tmp_file_path = Some(tmp_path);
            }
            _ => {}
        }
    }

    let tmp_path = tmp_file_path.ok_or((StatusCode::BAD_REQUEST, "missing file field".into()))?;
    let filename = filename
        .or_else(|| {
            // Fallback: derive from tmp path
            tmp_path.file_stem().map(|s| s.to_string_lossy().to_string())
        })
        .ok_or((StatusCode::BAD_REQUEST, "missing filename".into()))?;

    // Sanitize filename
    let filename = sanitize_filename(&filename);
    if filename.is_empty() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err((StatusCode::BAD_REQUEST, "invalid filename".into()));
    }

    // Acquire transcode semaphore — limits concurrent CPU load
    let _permit = state.transcode_sem.acquire().await.map_err(|_| {
        (StatusCode::SERVICE_UNAVAILABLE, "transcode unavailable".into())
    })?;

    // Transcode
    let result = crate::transcode::transcode(&tmp_path, &filename, &state.config.storage_path)
        .await
        .map_err(|e| {
            warn!("[upload] transcode failed for {filename}: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("transcode: {e}"))
        });

    // Always cleanup tmp
    let _ = tokio::fs::remove_file(&tmp_path).await;

    let result = result?;

    Ok(Json(UploadResponse {
        filename: filename.clone(),
        hq_path: format!("hq/{filename}.ogg"),
        sq_path: format!("sq/{filename}.ogg"),
        duration_secs: result.duration_secs,
    }))
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}
