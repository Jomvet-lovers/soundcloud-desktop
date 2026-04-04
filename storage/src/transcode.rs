use std::path::{Path, PathBuf};

use tokio::process::Command;
use tracing::{info, warn};

pub struct TranscodeResult {
    pub duration_secs: f64,
}

/// Transcode input file to Opus HQ (256k) + SQ (128k).
/// Returns paths to the two output files in storage_path.
pub async fn transcode(
    input: &Path,
    filename: &str,
    storage_path: &str,
) -> Result<TranscodeResult, TranscodeError> {
    let hq_dir = PathBuf::from(storage_path).join("hq");
    let sq_dir = PathBuf::from(storage_path).join("sq");
    tokio::fs::create_dir_all(&hq_dir).await?;
    tokio::fs::create_dir_all(&sq_dir).await?;

    let ogg_name = format!("{filename}.ogg");
    let hq_path = hq_dir.join(&ogg_name);
    let sq_path = sq_dir.join(&ogg_name);

    // Probe duration first
    let duration_secs = probe_duration(input).await.unwrap_or(0.0);

    // Single ffmpeg call: two outputs from one input
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            input.to_str().unwrap_or_default(),
            // HQ: Opus 256kbps
            "-map", "0:a:0",
            "-c:a", "libopus",
            "-b:a", "256k",
            "-vbr", "on",
            "-compression_level", "10",
            "-application", "audio",
            hq_path.to_str().unwrap_or_default(),
            // SQ: Opus 128kbps
            "-map", "0:a:0",
            "-c:a", "libopus",
            "-b:a", "128k",
            "-vbr", "on",
            "-compression_level", "10",
            "-application", "audio",
            sq_path.to_str().unwrap_or_default(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?
        .wait()
        .await?;

    if !status.success() {
        // Cleanup partial files
        let _ = tokio::fs::remove_file(&hq_path).await;
        let _ = tokio::fs::remove_file(&sq_path).await;
        return Err(TranscodeError::FfmpegFailed(status.code().unwrap_or(-1)));
    }

    info!(
        "[transcode] {filename} → HQ {:.1}MB, SQ {:.1}MB, {:.1}s",
        file_size_mb(&hq_path).await,
        file_size_mb(&sq_path).await,
        duration_secs,
    );

    Ok(TranscodeResult { duration_secs })
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

async fn probe_duration(path: &Path) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-show_entries", "format=duration",
            "-of", "csv=p=0",
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
    #[error("ffmpeg exited with code {0}")]
    FfmpegFailed(i32),
}

impl From<TranscodeError> for axum::http::StatusCode {
    fn from(_: TranscodeError) -> Self {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}
