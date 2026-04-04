use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio_util::io::ReaderStream;
use tracing::info;

use crate::AppState;

/// GET /{path} — serve files (public, no auth)
pub async fn serve(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Response, StatusCode> {
    if path.contains("..") || path.starts_with('/') {
        return Err(StatusCode::FORBIDDEN);
    }

    let file_path = PathBuf::from(&state.config.storage_path).join(&path);

    let file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let metadata = file
        .metadata()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let content_type = if path.ends_with(".ogg") {
        "audio/ogg"
    } else if path.ends_with(".mp3") {
        "audio/mpeg"
    } else {
        "application/octet-stream"
    };

    let stream = ReaderStream::new(file);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .body(Body::from_stream(stream))
        .unwrap())
}

/// DELETE /files/{filename} — delete both HQ and SQ
pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(filename): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing token".into()))?;

    if token != state.config.admin_token {
        return Err((StatusCode::FORBIDDEN, "invalid token".into()));
    }

    if filename.contains("..") || filename.contains('/') {
        return Err((StatusCode::BAD_REQUEST, "invalid filename".into()));
    }

    crate::transcode::delete_files(&filename, &state.config.storage_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("delete: {e}")))?;

    info!("[files] deleted {filename}");
    Ok(StatusCode::OK)
}
