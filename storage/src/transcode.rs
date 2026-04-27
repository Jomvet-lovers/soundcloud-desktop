use std::path::{Path, PathBuf};

use tokio::process::Command;
use uuid::Uuid;

pub const MIN_UPLOAD_DURATION_SECS: f64 = 30.0;

#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
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

/// Готовит пару tmp-путей для HQ и SQ выхода ffmpeg-а в `result_dir`.
pub fn stage_outputs(result_dir: &str, filename: &str) -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(result_dir);
    let id = Uuid::new_v4();
    (
        dir.join(format!(".{filename}.hq.{id}.tmp.ogg")),
        dir.join(format!(".{filename}.sq.{id}.tmp.ogg")),
    )
}

pub async fn probe_duration(path: &Path, ffprobe_bin: &str) -> Option<f64> {
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
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Один ffmpeg-вызов с N входами и 2N выходами (HQ+SQ Opus).
/// Все треки декодятся в одном процессе — экономим старт ffmpeg / линковку libopus / парсинг argv.
pub async fn run_ffmpeg_batch(
    ffmpeg_bin: &str,
    inputs: &[&Path],
    outputs: &[(PathBuf, PathBuf)],
) -> Result<(), TranscodeError> {
    debug_assert_eq!(inputs.len(), outputs.len());

    let mut cmd = Command::new(ffmpeg_bin);
    cmd.args(["-v", "error", "-hide_banner", "-nostats", "-y"]);

    for input in inputs {
        cmd.arg("-i").arg(input);
    }

    let mappings: Vec<String> = (0..inputs.len()).map(|i| format!("{i}:a:0")).collect();

    for (idx, (hq, sq)) in outputs.iter().enumerate() {
        let mapping = &mappings[idx];
        cmd.args([
            "-map",
            mapping,
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
        ]);
        cmd.arg(hq);
        cmd.args([
            "-map",
            mapping,
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
        ]);
        cmd.arg(sq);
    }

    let output = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await?;

    if !output.status.success() {
        for (hq, sq) in outputs {
            let _ = tokio::fs::remove_file(hq).await;
            let _ = tokio::fs::remove_file(sq).await;
        }
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
    Ok(())
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
