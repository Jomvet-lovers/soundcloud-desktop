//! Track upload commands: local validation, one-time grant issuance, and the
//! multipart POST driven through the hidden writer WebView.
//!
//! The frontend never sends audio bytes through Tauri IPC. `upload_inspect`
//! checks the file up front, `upload_issue_token` registers a one-time grant
//! and returns a loopback URL, and `track_upload` tells the writer page to
//! `fetch()` that URL and POST the multipart body to SoundCloud. The writer
//! only ever sees the token URL; file paths stay inside this process.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Manager;

use crate::app::diagnostics::log_native;
use crate::auth::SessionStore;
use crate::network::server::ServerState;
use crate::network::upload_grants;
use crate::rt::AppHandle;
use crate::track_cache::transcode;

use super::{sc, webview, DirectState};

/// SoundCloud rejects request bodies above 500 MB; the UI blocks files above it.
const SOFT_MAX_BYTES: u64 = 500 * 1024 * 1024;
/// Hard file-size limit (SoundCloud documents 4 GB).
const HARD_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Hard duration limit (24 h); the UI refuses anything longer.
const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1000;
/// Source formats SoundCloud accepts, plus the common derivatives.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "aac", "flac", "wav", "ogg", "opus", "aiff", "aif", "wma", "mp2", "amr",
];
const ARTWORK_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
/// Response-body characters kept in the failure message.
const ERROR_BODY_MAX: usize = 500;

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

#[derive(Deserialize)]
pub struct UploadMeta {
    pub title: String,
    pub artist: Option<String>,
    pub genre: Option<String>,
    #[serde(alias = "tagList")]
    pub tag_list: Option<String>,
    pub description: Option<String>,
    pub sharing: Option<String>,
    #[serde(alias = "embeddableBy")]
    pub embeddable_by: Option<String>,
    pub permalink: Option<String>,
    #[serde(alias = "releaseDate")]
    pub release_date: Option<String>,
    pub downloadable: Option<bool>,
    pub commentable: Option<bool>,
}

#[derive(Serialize)]
pub struct UploadedTrack {
    pub id: u64,
    pub urn: String,
    pub permalink_url: String,
    pub title: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct UploadToken {
    /// The grant map key; callers may also just use `url`.
    pub token: String,
    pub url: String,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
}

/// Outcome of the pure pre-upload checks (see [`inspect_verdict`]).
struct Verdict {
    too_large: bool,
    too_long: bool,
    warnings: Vec<String>,
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Loopback URL the writer page reads the granted bytes from.
fn grant_url(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/up/{token}")
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
    let ffmpeg = transcode::acquire_ffmpeg(&ffmpeg_dir(app)?).await?;
    transcode::probe_duration_ms(&ffmpeg, path).await
}

/// File facts + the pure verdict, so the command body stays thin.
#[tauri::command]
pub async fn upload_inspect(app: AppHandle, path: String) -> Result<AudioInspection, String> {
    let path = PathBuf::from(path);
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|_| "File not found".to_string())?;
    if !meta.is_file() {
        return Err("File not found".into());
    }
    let extension = extension_of(&path);
    if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!("Unsupported audio format: .{extension}"));
    }
    let size = meta.len();
    let duration_ms = probe_duration(&app, &path).await;
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

/// Track attributes for the JSON create call. `title` is required; optional
/// text fields are dropped when absent or empty. `downloadable` / `commentable`
/// default to SoundCloud's `true` when the caller omits them.
pub(crate) fn track_meta_json(meta: &UploadMeta) -> Result<Value, String> {
    if meta.title.trim().is_empty() {
        return Err("Title is required".into());
    }
    if let Some(sharing) = meta.sharing.as_deref().filter(|s| !s.is_empty()) {
        if sharing != "public" && sharing != "private" {
            return Err(format!("Invalid sharing setting: {sharing}"));
        }
    }
    let mut obj = serde_json::Map::new();
    obj.insert("title".into(), json!(meta.title));
    let texts: [(&str, Option<&String>); 7] = [
        ("genre", meta.genre.as_ref()),
        ("tag_list", meta.tag_list.as_ref()),
        ("description", meta.description.as_ref()),
        ("sharing", meta.sharing.as_ref()),
        // The web app sends `embeddable` (the model attribute is
        // `embeddable_by`, but the save payload renames it).
        ("embeddable", meta.embeddable_by.as_ref()),
        ("permalink", meta.permalink.as_ref()),
        ("release_date", meta.release_date.as_ref()),
    ];
    for (key, value) in texts {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            obj.insert(key.into(), json!(value));
        }
    }
    if let Some(artist) = meta.artist.as_ref().filter(|v| !v.trim().is_empty()) {
        obj.insert("publisher_metadata".into(), json!({ "artist": artist }));
    }
    obj.insert(
        "downloadable".into(),
        json!(meta.downloadable.unwrap_or(true)),
    );
    obj.insert("commentable".into(), json!(meta.commentable.unwrap_or(true)));
    Ok(Value::Object(obj))
}

/// Map the upload response into the wire shape the frontend consumes. The
/// writer channel truncates long payloads, so a regex recovers the id from a
/// cut-off body (same fallback as `routes/normalize.rs`).
pub(crate) fn parse_uploaded_track(payload: &str) -> Result<UploadedTrack, String> {
    if let Ok(v) = serde_json::from_str::<Value>(payload) {
        if let Some(id) = v.get("id").and_then(Value::as_u64) {
            let text = |key: &str| {
                v.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let state = text("state");
            return Ok(UploadedTrack {
                id,
                urn: v
                    .get("urn")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("soundcloud:tracks:{id}")),
                permalink_url: text("permalink_url"),
                title: text("title"),
                state: if state.is_empty() {
                    "processing".into()
                } else {
                    state
                },
            });
        }
    }
    match id_from_payload(payload) {
        Some(id) => Ok(UploadedTrack {
            id,
            urn: format!("soundcloud:tracks:{id}"),
            permalink_url: String::new(),
            title: String::new(),
            state: "processing".into(),
        }),
        None => Err("Track ID was not found in the response".into()),
    }
}

fn id_from_payload(payload: &str) -> Option<u64> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r#""id"\s*:\s*(\d+)"#).expect("static pattern"));
    re.captures(payload)?.get(1)?.as_str().parse().ok()
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max).collect();
    out.push('…');
    out
}

/// Longest id the frontend may supply; generated UUIDs are well below it.
const UPLOAD_ID_MAX: usize = 64;

/// The raw response body when the writer channel preserved a complete Track
/// object. Truncated payloads fall back to the regex-parsed id.
fn uploaded_track_object(payload: &str) -> Option<Value> {
    serde_json::from_str::<Value>(payload)
        .ok()
        .filter(|v| v.get("id").and_then(Value::as_u64).is_some())
}

/// Store entry for a finished upload. The raw Track is preferred so the list
/// keeps artwork and user data; otherwise the parsed fields are enough for the
/// `/me/tracks` overlay.
fn recorded_uploaded_track(track: &UploadedTrack, raw: Option<Value>) -> Value {
    match raw.filter(|v| v.get("id").and_then(Value::as_u64).is_some()) {
        Some(raw) => raw,
        None => json!({
            "id": track.id,
            "urn": track.urn,
            "permalink_url": track.permalink_url,
            "title": track.title,
            "state": track.state,
            "uploaded_by_app": true,
        }),
    }
}

/// Best-effort local record, written to the active account's store. It returns
/// `()` on purpose: a missing owner or a store problem must never turn a
/// successful upload into an error.
async fn remember_uploaded_track(
    app: &AppHandle,
    token: &str,
    track: &UploadedTrack,
    raw: Option<Value>,
) {
    let direct = app.state::<Arc<DirectState>>().inner().clone();
    let Some(owner) = direct.resolve_owner(Some(token)).await else {
        log_native(
            app,
            "WARN",
            "[upload] account could not be resolved; track not recorded locally",
        );
        return;
    };
    let store = direct.store_for(Some(owner)).await;
    let mut store = store.lock().await;
    store.record_uploaded_track(recorded_uploaded_track(track, raw));
    store.save();
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

/// Abort the in-flight XHR for `upload_id` (from a `track:upload-progress`
/// event). Unknown ids are a no-op; the upload may already be finished.
#[tauri::command]
pub async fn upload_cancel(app: AppHandle, upload_id: String) -> Result<(), String> {
    webview::abort_upload(&app, &upload_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(value: Value) -> UploadMeta {
        serde_json::from_value(value).expect("meta should deserialize")
    }

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

    #[test]
    fn track_meta_requires_title() {
        assert!(track_meta_json(&meta(json!({"title": ""}))).is_err());
        assert!(track_meta_json(&meta(json!({"title": "   "}))).is_err());
        let value = track_meta_json(&meta(json!({"title": "Track"}))).unwrap();
        assert_eq!(value["title"], "Track");
    }

    #[test]
    fn track_meta_skips_absent_and_empty_optionals() {
        let value = track_meta_json(&meta(json!({
            "title": "T",
            "artist": "",
            "genre": null,
            "description": null
        })))
        .unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["commentable", "downloadable", "title"]);
    }

    #[test]
    fn track_meta_includes_provided_values() {
        let value = track_meta_json(&meta(json!({
            "title": "T",
            "artist": "A",
            "genre": "Techno",
            "tag_list": "a,b",
            "description": "d",
            "sharing": "private",
            "embeddable_by": "me",
            "permalink": "my-track",
            "release_date": "2026-10-19"
        })))
        .unwrap();
        for (key, expected) in [
            ("title", "T"),
            ("genre", "Techno"),
            ("tag_list", "a,b"),
            ("description", "d"),
            ("sharing", "private"),
            ("embeddable", "me"),
            ("permalink", "my-track"),
            ("release_date", "2026-10-19"),
        ] {
            assert_eq!(value[key], expected, "missing {key}");
        }
        assert_eq!(value["publisher_metadata"]["artist"], "A");
    }

    #[test]
    fn track_meta_defaults_flags_to_true() {
        let value = track_meta_json(&meta(json!({"title": "T"}))).unwrap();
        assert_eq!(value["downloadable"], true);
        assert_eq!(value["commentable"], true);

        let off = track_meta_json(&meta(json!({
            "title": "T",
            "downloadable": false,
            "commentable": false
        })))
        .unwrap();
        assert_eq!(off["downloadable"], false);
        assert_eq!(off["commentable"], false);
    }

    #[test]
    fn track_meta_rejects_bad_sharing() {
        assert!(track_meta_json(&meta(json!({"title": "T", "sharing": "unlisted"}))).is_err());
        // The empty string is the omitted-value convention, not an error.
        assert!(track_meta_json(&meta(json!({"title": "T", "sharing": ""}))).is_ok());
    }

    #[test]
    fn track_meta_accepts_camel_case_keys() {
        let value = track_meta_json(&meta(json!({
            "title": "T",
            "tagList": "a,b",
            "embeddableBy": "me",
            "releaseDate": "2026-10-19"
        })))
        .unwrap();
        assert_eq!(value["tag_list"], "a,b");
        assert_eq!(value["embeddable"], "me");
        assert_eq!(value["release_date"], "2026-10-19");
    }

    #[test]
    fn parse_uploaded_track_reads_json() {
        let track = parse_uploaded_track(
            r#"{"id":42,"urn":"soundcloud:tracks:42","permalink_url":"https://soundcloud.com/x/y","title":"T","state":"processing"}"#,
        )
        .unwrap();
        assert_eq!(track.id, 42);
        assert_eq!(track.urn, "soundcloud:tracks:42");
        assert_eq!(track.permalink_url, "https://soundcloud.com/x/y");
        assert_eq!(track.title, "T");
        assert_eq!(track.state, "processing");
    }

    #[test]
    fn parse_uploaded_track_fills_defaults() {
        let track = parse_uploaded_track(r#"{"id":7,"title":"X"}"#).unwrap();
        assert_eq!(track.urn, "soundcloud:tracks:7");
        assert_eq!(track.permalink_url, "");
        assert_eq!(track.title, "X");
        assert_eq!(track.state, "processing");
    }

    #[test]
    fn parse_uploaded_track_regex_fallback_for_truncated_json() {
        let track = parse_uploaded_track(r#"{"id": 12345, "title":"cut off mid..."#).unwrap();
        assert_eq!(track.id, 12345);
        assert_eq!(track.urn, "soundcloud:tracks:12345");
        assert_eq!(track.title, "");
        assert_eq!(track.state, "processing");
    }

    #[test]
    fn parse_uploaded_track_without_id_is_err() {
        assert!(parse_uploaded_track(r#"{"error":"bad request"}"#).is_err());
        assert!(parse_uploaded_track("not json").is_err());
    }

    fn parsed_track(id: u64) -> UploadedTrack {
        UploadedTrack {
            id,
            urn: format!("soundcloud:tracks:{id}"),
            permalink_url: format!("https://soundcloud.com/me/track-{id}"),
            title: "Track".into(),
            state: "processing".into(),
        }
    }

    #[test]
    fn raw_upload_payload_is_kept_only_for_a_complete_track() {
        assert!(uploaded_track_object(r#"{"id":1,"title":"x"}"#).is_some());
        assert!(uploaded_track_object(r#"{"id":1,"title":"cut off"#).is_none());
        assert!(uploaded_track_object(r#"{"error":"bad request"}"#).is_none());
    }

    #[test]
    fn recorded_upload_prefers_the_raw_track_object() {
        let raw = json!({
            "id": 42,
            "title": "raw",
            "artwork_url": "https://example.com/art.jpg",
        });
        let value = recorded_uploaded_track(&parsed_track(42), Some(raw));

        assert_eq!(
            value.get("artwork_url").and_then(Value::as_str),
            Some("https://example.com/art.jpg"),
        );
        let mut store = crate::direct::store::LocalStore::default();
        store.record_uploaded_track(value);
        assert_eq!(store.uploaded_tracks.len(), 1);
    }

    /// A truncated or non-object body must fall back to the parsed fields
    /// instead of failing the already-successful upload.
    #[test]
    fn recorded_upload_falls_back_when_the_raw_body_is_unusable() {
        for raw in [
            None,
            Some(json!(["not", "an", "object"])),
            Some(json!({ "title": "no id" })),
        ] {
            let value = recorded_uploaded_track(&parsed_track(7), raw);
            assert_eq!(value.get("id").and_then(Value::as_u64), Some(7));
            assert_eq!(
                value.get("urn").and_then(Value::as_str),
                Some("soundcloud:tracks:7"),
            );
            assert_eq!(
                value.get("uploaded_by_app").and_then(Value::as_bool),
                Some(true),
            );
        }
    }

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
