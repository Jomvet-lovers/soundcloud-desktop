use std::path::{Path, PathBuf};

use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

const MIN_UPLOAD_DURATION_SECS: f64 = 30.0;

pub struct TranscodeResult {
    pub duration_secs: f64,
}

/// Transcode input file to Opus HQ (256k) + SQ (128k).
/// Returns paths to the two output files in storage_path.
pub async fn transcode(
    input: &Path,
    filename: &str,
    storage_path: &str,
    ffmpeg_bin: &str,
    ffprobe_bin: &str,
) -> Result<TranscodeResult, TranscodeError> {
    let hq_dir = PathBuf::from(storage_path).join("hq");
    let sq_dir = PathBuf::from(storage_path).join("sq");
    tokio::fs::create_dir_all(&hq_dir).await?;
    tokio::fs::create_dir_all(&sq_dir).await?;

    let ogg_name = format!("{filename}.ogg");
    let hq_path = hq_dir.join(&ogg_name);
    let sq_path = sq_dir.join(&ogg_name);
    let hq_tmp_path = temp_output_path(&hq_dir, filename, "hq");
    let sq_tmp_path = temp_output_path(&sq_dir, filename, "sq");

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
        // Cleanup partial files
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

    if let Err(err) = replace_file(&hq_tmp_path, &hq_path).await {
        cleanup_file(&hq_tmp_path).await;
        cleanup_file(&sq_tmp_path).await;
        return Err(err);
    }

    if let Err(err) = replace_file(&sq_tmp_path, &sq_path).await {
        cleanup_file(&sq_tmp_path).await;
        return Err(err);
    }

    info!(
        "[transcode] {filename} → HQ {:.1}MB, SQ {:.1}MB, {:.1}s",
        file_size_mb(&hq_path).await,
        file_size_mb(&sq_path).await,
        duration_secs,
    );

    Ok(TranscodeResult { duration_secs })
}

fn temp_output_path(dir: &Path, filename: &str, quality: &str) -> PathBuf {
    dir.join(format!(".{filename}.{}.{}.tmp", quality, Uuid::new_v4()))
}

async fn cleanup_file(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}

async fn replace_file(src: &Path, dst: &Path) -> Result<(), TranscodeError> {
    match tokio::fs::rename(src, dst).await {
        Ok(()) => Ok(()),
        Err(first_err) => {
            if tokio::fs::metadata(dst).await.is_ok() {
                tokio::fs::remove_file(dst).await?;
                tokio::fs::rename(src, dst).await?;
                Ok(())
            } else {
                Err(TranscodeError::Io(first_err))
            }
        }
    }
}

/// Delete both HQ and SQ files for a given filename.
pub async fn delete_files(filename: &str, storage_path: &str) -> Result<(), TranscodeError> {
    let ogg_name = format!("{filename}.ogg");
    let hq = PathBuf::from(storage_path).join("hq").join(&ogg_name);
    let sq = PathBuf::from(storage_path).join("sq").join(&ogg_name);

    let mut deleted = false;
    if hq.exists() {
        tokio::fs::remove_file(&hq).await?;
        deleted = true;
    }
    if sq.exists() {
        tokio::fs::remove_file(&sq).await?;
        deleted = true;
    }

    if deleted {
        info!("[transcode] deleted {filename}");
    } else {
        warn!("[transcode] {filename} not found for deletion");
    }

    Ok(())
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
