//! Pre-upload validation: file facts, format whitelist, duration probing.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Manager;

use crate::rt::AppHandle;
use crate::track_cache::transcode;

/// SoundCloud's documented request-body cap; the UI blocks files above it.
pub(super) const SOFT_MAX_BYTES: u64 = crate::network::upload_grants::MAX_BYTES;
/// Hard file-size limit (SoundCloud documents 4 GB).
const HARD_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Hard duration limit (24 h); the UI refuses anything longer.
const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1000;
/// Source formats SoundCloud accepts, plus the common derivatives.
pub(super) const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "aac", "flac", "wav", "ogg", "opus", "aiff", "aif", "wma", "mp2", "amr",
];
pub(super) const ARTWORK_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

#[derive(Serialize)]
pub struct AudioInspection {
    pub size: u64,
    pub filename: String,
    pub duration_ms: Option<u64>,
    pub extension_ok: bool,
    /// Above the 500 MB request-body limit: the UI refuses the upload.
    pub too_large: bool,
    /// Above 24 h: the UI refuses the upload.
    pub too_long: bool,
    pub warnings: Vec<String>,
}

/// Outcome of the pure pre-upload checks (see [`inspect_verdict`]).
struct Verdict {
    too_large: bool,
    too_long: bool,
    warnings: Vec<String>,
}

pub(super) fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Shared existence check for the inspect and token commands.
pub(super) async fn file_meta(path: &Path) -> Result<std::fs::Metadata, String> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|_| "File not found".to_string())?;
    if !meta.is_file() {
        return Err("File not found".into());
    }
    Ok(meta)
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `Err` above the hard limit, otherwise the soft-limit warnings.
fn inspect_verdict(size: u64, duration_ms: Option<u64>) -> Result<Verdict, String> {
    if size > HARD_MAX_BYTES {
        return Err("File is larger than SoundCloud's 4 GB limit".into());
    }
    let mut warnings = Vec::new();
    if size > SOFT_MAX_BYTES {
        warnings.push("File is larger than SoundCloud's 500 MB upload limit".into());
    }
    let too_long = duration_ms.is_some_and(|ms| ms > MAX_DURATION_MS);
    if too_long {
        warnings.push("Track is longer than 24 hours".into());
    } else if duration_ms.is_none() {
        warnings.push("Duration could not be determined".into());
    }
    Ok(Verdict {
        too_large: size > SOFT_MAX_BYTES,
        too_long,
        warnings,
    })
}

/// Same directory `track_cache::state::init_ffmpeg` used at startup, so the
/// already-acquired binary is reused instead of a second download.
fn ffmpeg_dir(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_cache_dir().ok()?.join("ffmpeg"))
}

/// `None` when ffmpeg cannot be resolved or the duration line cannot be parsed.
async fn probe_duration(app: &AppHandle, path: &Path) -> Option<u64> {
    let ffmpeg = transcode::acquire_ffmpeg(&ffmpeg_dir(app)?).await.ok()?;
    transcode::probe_duration_ms(&ffmpeg, path).await
}

/// File facts + the pure verdict, so the command body stays thin.
pub(super) async fn inspect(app: &AppHandle, path: String) -> Result<AudioInspection, String> {
    let path = PathBuf::from(path);
    let meta = file_meta(&path).await?;
    let extension = extension_of(&path);
    if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!("Unsupported audio format: .{extension}"));
    }
    let size = meta.len();
    let duration_ms = probe_duration(app, &path).await;
    let verdict = inspect_verdict(size, duration_ms)?;
    Ok(AudioInspection {
        size,
        filename: file_name(&path),
        duration_ms,
        extension_ok: true,
        too_large: verdict.too_large,
        too_long: verdict.too_long,
        warnings: verdict.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_extensions_are_allowed() {
        for ext in AUDIO_EXTENSIONS {
            assert!(AUDIO_EXTENSIONS.contains(ext), "{ext}");
            assert_eq!(extension_of(Path::new(&format!("track.{ext}"))), *ext);
        }
        assert_eq!(extension_of(Path::new("TRACK.MP3")), "mp3");
        assert_eq!(extension_of(Path::new("no-extension")), "");
    }

    #[test]
    fn unknown_audio_extensions_are_rejected() {
        for name in ["notes.txt", "movie.mp4", "track.mp3.bak", "cover.jpg"] {
            let ext = extension_of(Path::new(name));
            assert!(!AUDIO_EXTENSIONS.contains(&ext.as_str()), "{name}");
        }
    }

    #[test]
    fn size_thresholds() {
        let ok = inspect_verdict(10 * 1024 * 1024, Some(1_000)).unwrap();
        assert!(!ok.too_large && !ok.too_long && ok.warnings.is_empty());

        let soft = inspect_verdict(SOFT_MAX_BYTES + 1, Some(1_000)).unwrap();
        assert!(soft.too_large && !soft.too_long);
        assert_eq!(
            soft.warnings,
            ["File is larger than SoundCloud's 500 MB upload limit"]
        );

        let at_boundary = inspect_verdict(SOFT_MAX_BYTES, Some(1_000)).unwrap();
        assert!(at_boundary.warnings.is_empty());
        assert!(inspect_verdict(HARD_MAX_BYTES, Some(1_000)).is_ok());
        assert_eq!(
            inspect_verdict(HARD_MAX_BYTES + 1, Some(1_000))
                .err()
                .as_deref(),
            Some("File is larger than SoundCloud's 4 GB limit")
        );
    }

    #[test]
    fn duration_thresholds() {
        let fine = inspect_verdict(1_000, Some(3_600_000)).unwrap();
        assert!(!fine.too_long && fine.warnings.is_empty());

        let boundary = inspect_verdict(1_000, Some(MAX_DURATION_MS)).unwrap();
        assert!(!boundary.too_long);

        let long = inspect_verdict(1_000, Some(MAX_DURATION_MS + 1)).unwrap();
        assert!(long.too_long);
        assert_eq!(long.warnings, ["Track is longer than 24 hours"]);

        let unknown = inspect_verdict(1_000, None).unwrap();
        assert!(!unknown.too_long);
        assert_eq!(unknown.warnings, ["Duration could not be determined"]);
    }
}
