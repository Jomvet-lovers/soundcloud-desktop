use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use tracing::{info, warn};

use crate::backend::BackendError;
use crate::AppState;

fn validate_path(path: &str) -> Result<(), StatusCode> {
    if path.contains("..") || path.starts_with('/') {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}

/// GET /{path} — stream bytes to client (never redirect, even for S3 backend).
pub async fn serve(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Response, StatusCode> {
    validate_path(&path)?;

    let (info, stream) = match state.backend.stream(&path).await {
        Ok(v) => v,
        Err(BackendError::NotFound) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            warn!("[files] stream {path} failed: {e}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            info.content_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
        )
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::ACCEPT_RANGES, "bytes");
    if info.size > 0 {
        builder = builder.header(header::CONTENT_LENGTH, info.size);
    }

    Ok(builder.body(Body::from_stream(stream)).unwrap())
}

/// HEAD /{path} — existence + size check only (no body download from S3).
pub async fn head(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Response, StatusCode> {
    validate_path(&path)?;

    let info = match state.backend.head(&path).await {
        Ok(Some(info)) => info,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            warn!("[files] head {path} failed: {e}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            info.content_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
        )
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::ACCEPT_RANGES, "bytes");
    if info.size > 0 {
        builder = builder.header(header::CONTENT_LENGTH, info.size);
    }

    Ok(builder.body(Body::empty()).unwrap())
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

    let hq_key = crate::backend::key_for("hq", &filename);
    let sq_key = crate::backend::key_for("sq", &filename);

    let hq_deleted = state
        .backend
        .delete_file(&hq_key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("delete hq: {e}")))?;
    let sq_deleted = state
        .backend
        .delete_file(&sq_key)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("delete sq: {e}")))?;

    if hq_deleted || sq_deleted {
        info!("[files] deleted {filename}");
    } else {
        warn!("[files] {filename} not found for deletion");
    }

    Ok(StatusCode::OK)
}
