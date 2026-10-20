//! Track upload commands: local validation, one-time grant issuance, and the
//! web uploader flow driven through the hidden writer WebView.
//!
//! The frontend never sends audio bytes through Tauri IPC. `upload_inspect`
//! checks the file up front, `upload_issue_token` registers a one-time grant
//! and returns a loopback URL, and `track_upload` tells the writer page to
//! `fetch()` that URL and POST the track through SoundCloud's web uploader.
//! The writer only ever sees the token URL; file paths stay inside this process.

mod inspect;
mod meta;
mod record;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::Manager;

use crate::auth::SessionStore;
use crate::network::server::ServerState;
use crate::network::upload_grants;
use crate::rt::AppHandle;

use super::{sc, webview, DirectState};
use inspect::{ARTWORK_EXTENSIONS, AudioInspection, extension_of, file_name};
use meta::{parse_uploaded_track, track_meta_json};
use record::{remember_uploaded_track, uploaded_track_object};

pub use meta::{UploadMeta, UploadedTrack};

/// Response-body characters kept in the failure message.
const ERROR_BODY_MAX: usize = 500;

/// Longest id the frontend may supply; generated UUIDs are well below it.
const UPLOAD_ID_MAX: usize = 64;

#[derive(Serialize)]
pub struct UploadToken {
    /// The grant map key; callers may also just use `url`.
    pub token: String,
    pub url: String,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
}

/// Loopback URL the writer page reads the granted bytes from.
fn grant_url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/up/{token}")
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max).collect();
    out.push('…');
    out
}

/// The id echoed in `track:upload-progress` and handed back to `upload_cancel`.
/// The frontend may pick its own so the cancel button works before the first
/// progress event arrives; `None` generates a UUID v4. Ids are restricted to
/// `[A-Za-z0-9_-]` (1..=64 chars) because they are interpolated into the
/// writer page's JavaScript.
pub(crate) fn resolve_upload_id(input: Option<&str>) -> Result<String, String> {
    let Some(id) = input else {
        return Ok(uuid::Uuid::new_v4().to_string());
    };
    let valid = !id.is_empty()
        && id.len() <= UPLOAD_ID_MAX
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(id.to_string())
    } else {
        Err("invalid upload id".into())
    }
}

/// File facts + the pure verdict; the heavy lifting lives in `inspect`.
#[tauri::command]
pub async fn upload_inspect(app: AppHandle, path: String) -> Result<AudioInspection, String> {
    inspect::inspect(&app, path).await
}

/// Register a one-time grant for `path` and return the loopback URL.
#[tauri::command]
pub async fn upload_issue_token(
    app: AppHandle,
    path: String,
    kind: String,
) -> Result<UploadToken, String> {
    let path = PathBuf::from(path);
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|_| "File not found".to_string())?;
    if !meta.is_file() {
        return Err("File not found".into());
    }
    if meta.len() > upload_grants::MAX_BYTES {
        return Err("The file exceeds SoundCloud's 500 MB upload limit".into());
    }
    let extension = extension_of(&path);
    match kind.as_str() {
        "audio" => {}
        "artwork" if !ARTWORK_EXTENSIONS.contains(&extension.as_str()) => {
            return Err(format!(
                "Artwork must be a JPG, PNG, or WebP image: .{extension}"
            ));
        }
        "artwork" => {}
        other => return Err(format!("Unknown upload kind: {other}")),
    }
    let filename = file_name(&path);
    let content_type = upload_grants::content_type_for(&path).to_string();
    let token = upload_grants::issue(path, filename.clone(), content_type.clone());
    let port = app.state::<Arc<ServerState>>().proxy_port;
    let url = grant_url(port, &token);
    Ok(UploadToken {
        token,
        url,
        filename,
        content_type,
        size: meta.len(),
    })
}

/// Upload `meta` with the bytes behind `audio_token` (optional artwork behind
/// `artwork_token`). The caller issues the tokens right before, so a missing
/// grant means the file must be selected again.
///
/// `upload_id` is the id this upload reports in `track:upload-progress` and
/// accepts in `upload_cancel`; pass the id the UI already tracks, or `None` to
/// let the backend generate one. Tauri v2 maps command argument names from
/// snake_case to camelCase, so the frontend invokes this with
/// `{ meta, audioToken, artworkToken, uploadId }`.
#[tauri::command]
pub async fn track_upload(
    app: AppHandle,
    meta: UploadMeta,
    audio_token: String,
    artwork_token: Option<String>,
    upload_id: Option<String>,
) -> Result<UploadedTrack, String> {
    let upload_id = resolve_upload_id(upload_id.as_deref())?;
    let token = app
        .state::<Arc<SessionStore>>()
        .active_token()
        .await
        .ok_or_else(|| "You are not signed in".to_string())?;

    let Some(audio) = upload_grants::peek(&audio_token) else {
        return Err("The upload token is invalid or expired. Select the file again".into());
    };
    // Artwork is optional: a dead artwork token must not sink the upload.
    let artwork = artwork_token.as_deref().and_then(upload_grants::peek);

    let track_meta = track_meta_json(&meta)?;
    let track_json = serde_json::to_string(&track_meta).map_err(|e| e.to_string())?;
    let direct = app.state::<Arc<DirectState>>().inner().clone();
    let client_id = direct.client_id().await?;
    let app_version = sc::app_version(&direct.http).await;
    let port = app.state::<Arc<ServerState>>().proxy_port;
    let audio_url = grant_url(port, &audio_token);
    let artwork_url = artwork
        .as_ref()
        .zip(artwork_token.as_deref())
        .map(|(_, token)| grant_url(port, token));
    let outcome = webview::do_upload(
        &app,
        &upload_id,
        &token,
        &client_id,
        &app_version,
        sc::SC_API,
        &track_json,
        &audio_url,
        &audio.filename,
        artwork_url.as_deref(),
        true,
    )
    .await?;

    if !(200..300).contains(&outcome.status) {
        return Err(format!(
            "Upload failed ({}): {}",
            outcome.status,
            truncate_chars(&outcome.payload, ERROR_BODY_MAX)
        ));
    }
    let track = parse_uploaded_track(&outcome.payload)?;
    remember_uploaded_track(
        &app,
        &token,
        &track,
        uploaded_track_object(&outcome.payload),
    )
    .await;
    Ok(track)
}

/// Abort the in-flight upload for `upload_id` (from a `track:upload-progress`
/// event). Unknown ids are a no-op; the upload may already be finished.
#[tauri::command]
pub async fn upload_cancel(app: AppHandle, upload_id: String) -> Result<(), String> {
    webview::abort_upload(&app, &upload_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_url_targets_the_loopback_proxy() {
        assert_eq!(
            grant_url(4321, "00000000-0000-4000-8000-000000000000"),
            "http://127.0.0.1:4321/up/00000000-0000-4000-8000-000000000000"
        );
    }

    #[test]
    fn truncate_chars_keeps_the_head() {
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
        assert_eq!(truncate_chars("ab", 3), "ab");
    }

    #[test]
    fn resolve_upload_id_generates_a_uuid_when_absent() {
        let generated = resolve_upload_id(None).unwrap();
        assert!(!generated.is_empty());
        assert!(generated.len() <= UPLOAD_ID_MAX);
        assert!(generated
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-'));
        // Each call picks a fresh id; sharing one would collide in `__upXhrs`.
        assert_ne!(resolve_upload_id(None).unwrap(), generated);
    }

    #[test]
    fn resolve_upload_id_rejects_invalid_input() {
        for bad in [
            "",
            " ",
            "has space",
            "with/slash",
            "trailing.dot",
            "café",
            "emoji🙂",
        ] {
            assert!(resolve_upload_id(Some(bad)).is_err(), "{bad:?}");
        }
        let too_long = "a".repeat(UPLOAD_ID_MAX + 1);
        assert!(resolve_upload_id(Some(&too_long)).is_err());
    }

    #[test]
    fn resolve_upload_id_keeps_the_supplied_value() {
        assert_eq!(
            resolve_upload_id(Some("upload-42_ABC")).unwrap(),
            "upload-42_ABC"
        );
        assert_eq!(
            resolve_upload_id(Some("00000000-0000-4000-8000-000000000000")).unwrap(),
            "00000000-0000-4000-8000-000000000000"
        );
        let boundary = "a".repeat(UPLOAD_ID_MAX);
        assert_eq!(resolve_upload_id(Some(&boundary)).unwrap(), boundary);
    }
}
