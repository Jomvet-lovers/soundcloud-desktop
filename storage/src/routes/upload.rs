use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Multipart, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use crate::pipeline::PipelineError;
use crate::AppState;

const TMP_RESCAN_COOLDOWN: Duration = Duration::from_secs(5);

#[derive(serde::Serialize)]
pub struct UploadResponse {
    pub filename: String,
    pub hq_path: String,
    pub sq_path: String,
    pub duration_secs: f64,
}

/// RAII guard — any reserved tmp bytes get released on drop, even on panic / early return.
struct TmpReservation<'a> {
    state: &'a AppState,
    bytes: u64,
}

impl<'a> TmpReservation<'a> {
    fn new(state: &'a AppState) -> Self {
        Self { state, bytes: 0 }
    }

    /// Try to reserve `n` more bytes. Returns false if reservation would exceed the limit;
    /// in that case the counter is rolled back and nothing is charged.
    fn try_add(&mut self, n: u64) -> bool {
        let Some(limit) = self.state.config.tmp_max_bytes else {
            self.bytes = self.bytes.saturating_add(n);
            return true;
        };
        let prev = self.state.tmp_used_bytes.fetch_add(n, Ordering::AcqRel);
        if prev.saturating_add(n) > limit {
            self.state.tmp_used_bytes.fetch_sub(n, Ordering::AcqRel);
            return false;
        }
        self.bytes = self.bytes.saturating_add(n);
        true
    }
}

impl Drop for TmpReservation<'_> {
    fn drop(&mut self) {
        if self.bytes > 0 && self.state.config.tmp_max_bytes.is_some() {
            self.state
                .tmp_used_bytes
                .fetch_sub(self.bytes, Ordering::AcqRel);
        }
    }
}

/// Reconcile the in-memory counter with the real filesystem state.
/// Needed to recover from external cleanup (manual `rm`, cron janitor) —
/// otherwise the counter stays artificially high forever.
async fn rescan_tmp_usage(state: &AppState) {
    let Ok(mut guard) = state.tmp_rescan_lock.try_lock() else {
        return;
    };
    if guard.elapsed() < TMP_RESCAN_COOLDOWN {
        return;
    }
    let pre = state.tmp_used_bytes.load(Ordering::Acquire);
    let disk = match dir_size_bytes(&state.config.source_path()).await {
        Ok(n) => n,
        Err(e) => {
            warn!("[tmp-rescan] walk failed: {e}");
            *guard = Instant::now();
            return;
        }
    };
    if pre > disk {
        let recovered = pre - disk;
        state
            .tmp_used_bytes
            .fetch_sub(recovered, Ordering::AcqRel);
        info!(
            "[tmp-rescan] recovered {:.2} MiB (counter {} → ~{} bytes)",
            recovered as f64 / (1024.0 * 1024.0),
            pre,
            pre - recovered,
        );
    }
    *guard = Instant::now();
}

pub async fn upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if state.config.disable_upload {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "upload disabled on this host".into(),
        ));
    }

    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing token".into()))?;

    if token != state.config.admin_token {
        return Err((StatusCode::FORBIDDEN, "invalid token".into()));
    }

    let mut filename: Option<String> = None;
    let mut tmp_file_path: Option<std::path::PathBuf> = None;
    let mut reservation = TmpReservation::new(&state);
    let source_dir = state.config.source_path();

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
                let id = uuid::Uuid::new_v4();
                let tmp_path =
                    std::path::PathBuf::from(&source_dir).join(format!("{id}.input"));

                let mut file = tokio::fs::File::create(&tmp_path).await.map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("create tmp: {e}"),
                    )
                })?;

                let mut stream = field;
                let mut total: u64 = 0;
                loop {
                    match stream.chunk().await {
                        Ok(Some(chunk)) => {
                            let n = chunk.len() as u64;
                            if !reservation.try_add(n) {
                                rescan_tmp_usage(&state).await;
                                if !reservation.try_add(n) {
                                    drop(file);
                                    let _ = tokio::fs::remove_file(&tmp_path).await;
                                    return Err((
                                        StatusCode::INSUFFICIENT_STORAGE,
                                        "tmp quota exceeded".into(),
                                    ));
                                }
                            }
                            total += n;
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

                file.flush()
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("flush: {e}")))?;
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
            tmp_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        })
        .ok_or((StatusCode::BAD_REQUEST, "missing filename".into()))?;

    let filename = sanitize_filename(&filename);
    if filename.is_empty() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err((StatusCode::BAD_REQUEST, "invalid filename".into()));
    }

    let file_lock = state.file_lock(&filename);
    let _file_guard = file_lock.lock().await;

    let result = state.pipeline.submit(tmp_path, filename.clone()).await;
    drop(reservation);

    let output = match result {
        Ok(out) => out,
        Err(PipelineError::TrackTooShort { duration_secs, .. }) => {
            info!("[upload] skipped short track {filename}: {duration_secs:.3}s");
            return Err((
                StatusCode::CONFLICT,
                format!("transcode skipped: short track ({duration_secs:.3}s)"),
            ));
        }
        Err(PipelineError::Ffmpeg(msg)) => {
            warn!("[upload] ffmpeg failed for {filename}: {msg}");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("transcode: {msg}")));
        }
        Err(PipelineError::Backend(msg)) => {
            warn!("[upload] backend failed for {filename}: {msg}");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("backend: {msg}")));
        }
        Err(PipelineError::Internal(msg)) => {
            warn!("[upload] internal failure for {filename}: {msg}");
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("internal: {msg}")));
        }
    };

    Ok(Json(UploadResponse {
        filename: filename.clone(),
        hq_path: format!("hq/{filename}.ogg"),
        sq_path: format!("sq/{filename}.ogg"),
        duration_secs: output.duration_secs,
    }))
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .collect::<String>()
        .trim_matches('.')
        .to_string()
}

/// Recursive walk — intended for one-shot startup seeding only. NEVER call on the hot path.
pub async fn dir_size_bytes(path: &str) -> std::io::Result<u64> {
    let mut total: u64 = 0;
    let mut stack: Vec<std::path::PathBuf> = vec![std::path::PathBuf::from(path)];
    while let Some(dir) = stack.pop() {
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        while let Some(entry) = rd.next_entry().await? {
            let ft = entry.file_type().await?;
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                if let Ok(meta) = entry.metadata().await {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    Ok(total)
}
