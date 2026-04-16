use std::path::{Path, PathBuf};

use tokio::process::Command;
use tracing::info;
use uuid::Uuid;

use crate::backend::{Backend, BackendError};

const MIN_UPLOAD_DURATION_SECS: f64 = 30.0;

pub struct TranscodeResult {
    pub duration_secs: f64,
}

/// Transcode input file to Opus HQ (256k) + SQ (128k) in tmp, then hand
/// both outputs to the backend for commit.
pub async fn transcode(
    input: &Path,
    filename: &str,
    backend: &Backend,
    tmp_path: &str,
    ffmpeg_bin: &str,
    ffprobe_bin: &str,
) -> Result<TranscodeResult, TranscodeError> {
    let tmp_dir = PathBuf::from(tmp_path);
    tokio::fs::create_dir_all(&tmp_dir).await?;

    let hq_tmp_path = temp_output_path(&tmp_dir, filename, "hq");
    let sq_tmp_path = temp_output_path(&tmp_dir, filename, "sq");

    // Probe duration first
    let duration_secs = probe_duration(input, ffprobe_bin).await.unwrap_or(0.0);
    if duration_secs > 0.0 && duration_secs <= MIN_UPLOAD_DURATION_SECS {
        return Err(TranscodeError::TrackTooShort {
            duration_secs,
            min_duration_secs: MIN_UPLOAD_DURATION_SECS,
        });
    }

    // Single ffmpeg call: two outputs from one input
    let output = Command::new(ffmpeg_bin)
        .args([
            "-v",
            "error",
            "-hide_banner",
            "-nostats",
            "-y",
            "-i",
            input.to_str().unwrap_or_default(),
            // HQ: Opus 256kbps
            "-map",
            "0:a:0",
            "-c:a",
            "libopus",
            "-b:a",
            "256k",
            "-vbr",
            "on",
            "-compression_level",
            "10",
            "-application",
            "audio",
            hq_tmp_path.to_str().unwrap_or_default(),
            // SQ: Opus 128kbps
            "-map",
            "0:a:0",
            "-c:a",
            "libopus",
            "-b:a",
            "128k",
            "-vbr",
            "on",
            "-compression_level",
            "10",
            "-application",
            "audio",
            sq_tmp_path.to_str().unwrap_or_default(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await?;

    if !output.status.success() {
        cleanup_file(&hq_tmp_path).await;
        cleanup_file(&sq_tmp_path).await;
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(TranscodeError::FfmpegFailed {
            code: output.status.code().unwrap_or(-1),
            stderr: if stderr.is_empty() {
                "unknown ffmpeg error".into()
            } else {
                stderr
            },
        });
    }

    let hq_size = file_size_mb(&hq_tmp_path).await;
    let sq_size = file_size_mb(&sq_tmp_path).await;

    let hq_key = crate::backend::key_for("hq", filename);
    if let Err(err) = backend
        .commit_transcode(&hq_key, &hq_tmp_path, ffprobe_bin, filename, "hq")
        .await
    {
        cleanup_file(&hq_tmp_path).await;
        cleanup_file(&sq_tmp_path).await;
        return Err(TranscodeError::Backend(err.to_string()));
    }

    let sq_key = crate::backend::key_for("sq", filename);
    if let Err(err) = backend
        .commit_transcode(&sq_key, &sq_tmp_path, ffprobe_bin, filename, "sq")
        .await
    {
        cleanup_file(&sq_tmp_path).await;
        return Err(TranscodeError::Backend(err.to_string()));
    }

    info!(
        "[transcode] {filename} → HQ {hq_size:.1}MB, SQ {sq_size:.1}MB, {duration_secs:.1}s"
    );

    Ok(TranscodeResult { duration_secs })
}

fn temp_output_path(dir: &Path, filename: &str, quality: &str) -> PathBuf {
    dir.join(format!(".{filename}.{}.{}.tmp.ogg", quality, Uuid::new_v4()))
}

async fn cleanup_file(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}

async fn probe_duration(path: &Path, ffprobe_bin: &str) -> Option<f64> {
    let output = Command::new(ffprobe_bin)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            path.to_str()?,
        ])
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse().ok()
}

async fn file_size_mb(path: &Path) -> f64 {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.len() as f64 / 1024.0 / 1024.0)
        .unwrap_or(0.0)
}

#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("track too short: {duration_secs:.3}s <= {min_duration_secs:.3}s")]
    TrackTooShort {
        duration_secs: f64,
        min_duration_secs: f64,
    },
    #[error("ffmpeg exited with code {code}: {stderr}")]
    FfmpegFailed { code: i32, stderr: String },
    #[error("backend: {0}")]
    Backend(String),
    #[error("{name} binary '{path}' is unavailable: {source}")]
    BinaryUnavailable {
        name: &'static str,
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{name} binary '{path}' exited with code {code}")]
    BinaryCheckFailed {
        name: &'static str,
        path: String,
        code: i32,
    },
}

impl From<BackendError> for TranscodeError {
    fn from(err: BackendError) -> Self {
        TranscodeError::Backend(err.to_string())
    }
}

impl From<TranscodeError> for axum::http::StatusCode {
    fn from(_: TranscodeError) -> Self {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}

pub async fn validate_binaries(ffmpeg_bin: &str, ffprobe_bin: &str) -> Result<(), TranscodeError> {
    validate_binary("ffmpeg", ffmpeg_bin).await?;
    validate_binary("ffprobe", ffprobe_bin).await?;
    Ok(())
}

async fn validate_binary(name: &'static str, path: &str) -> Result<(), TranscodeError> {
    let status = Command::new(path)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|source| TranscodeError::BinaryUnavailable {
            name,
            path: path.to_string(),
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(TranscodeError::BinaryCheckFailed {
            name,
            path: path.to_string(),
            code: status.code().unwrap_or(-1),
        })
    }
}
