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
use serde_json::Value;
use tauri::Manager;

use crate::auth::SessionStore;
use crate::network::server::ServerState;
use crate::network::upload_grants;
use crate::rt::AppHandle;
use crate::track_cache::transcode;

use super::webview;

/// SoundCloud rejects request bodies above 500 MB; the UI confirms first.
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
const TRACKS_ENDPOINT: &str = "https://api.soundcloud.com/tracks";
/// Response-body characters kept in the failure message.
const ERROR_BODY_MAX: usize = 500;

#[derive(Serialize)]
pub struct AudioInspection {
    pub size: u64,
    pub filename: String,
    pub duration_ms: Option<u64>,
    pub extension_ok: bool,
    /// Above the 500 MB request-body limit: continue only after a UI confirm.
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
        return Err("ファイルが大きすぎます(4GB まで)".into());
    }
    let mut warnings = Vec::new();
    if size > SOFT_MAX_BYTES {
        warnings.push("500MB を超えています。アップロードに失敗することがあります".into());
    }
    let too_long = duration_ms.is_some_and(|ms| ms > MAX_DURATION_MS);
    if too_long {
        warnings.push("24 時間を超える曲はアップロードできません".into());
    } else if duration_ms.is_none() {
        warnings.push("長さを取得できませんでした".into());
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
        .map_err(|_| "ファイルが見つかりません".to_string())?;
    if !meta.is_file() {
        return Err("ファイルが見つかりません".into());
    }
    let extension = extension_of(&path);
    if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err(format!("対応形式ではありません: .{extension}"));
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
        .map_err(|_| "ファイルが見つかりません".to_string())?;
    if !meta.is_file() {
        return Err("ファイルが見つかりません".into());
    }
    if meta.len() > upload_grants::MAX_BYTES {
        return Err("ファイルが大きすぎます(500MB まで)".into());
    }
    let extension = extension_of(&path);
    match kind.as_str() {
        "audio" => {}
        "artwork" if !ARTWORK_EXTENSIONS.contains(&extension.as_str()) => {
            return Err(format!(
                "アートワークは JPG / PNG / WebP のみ対応です: .{extension}"
            ));
        }
        "artwork" => {}
        other => return Err(format!("不明な種別です: {other}")),
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

/// `track[title]` is required; optional text fields are dropped when absent or
/// empty. `downloadable` / `commentable` default to SoundCloud's `true` when
/// the caller omits them.
pub(crate) fn meta_fields(meta: &UploadMeta) -> Result<Vec<(String, String)>, String> {
    if meta.title.trim().is_empty() {
        return Err("タイトルを入力してください".into());
    }
    if let Some(sharing) = meta.sharing.as_deref().filter(|s| !s.is_empty()) {
        if sharing != "public" && sharing != "private" {
            return Err(format!("公開設定が不正です: {sharing}"));
        }
    }
    let mut fields = vec![("track[title]".to_string(), meta.title.clone())];
    let texts: [(&str, Option<&String>); 8] = [
        ("track[artist]", meta.artist.as_ref()),
        ("track[genre]", meta.genre.as_ref()),
        ("track[tag_list]", meta.tag_list.as_ref()),
        ("track[description]", meta.description.as_ref()),
        ("track[sharing]", meta.sharing.as_ref()),
        ("track[embeddable_by]", meta.embeddable_by.as_ref()),
        ("track[permalink]", meta.permalink.as_ref()),
        ("track[release_date]", meta.release_date.as_ref()),
    ];
    for (key, value) in texts {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            fields.push((key.to_string(), value.clone()));
        }
    }
    fields.push((
        "track[downloadable]".into(),
        flag_field(meta.downloadable).into(),
    ));
    fields.push((
        "track[commentable]".into(),
        flag_field(meta.commentable).into(),
    ));
    Ok(fields)
}

/// `downloadable` / `commentable` serialize as `"true"` when omitted, matching
/// SoundCloud's own default.
fn flag_field(value: Option<bool>) -> &'static str {
    if value.unwrap_or(true) {
        "true"
    } else {
        "false"
    }
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
        None => Err("応答からトラック ID を取得できませんでした".into()),
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
        .ok_or_else(|| "ログインしていません".to_string())?;

    let Some(audio) = upload_grants::peek(&audio_token) else {
        return Err(
            "アップロード用のトークンが無効か期限切れです。ファイルを選び直してください".into(),
        );
    };
    // Artwork is optional: a dead artwork token must not sink the upload.
    let artwork = artwork_token.as_deref().and_then(upload_grants::peek);

    let fields = meta_fields(&meta)?;
    let port = app.state::<Arc<ServerState>>().proxy_port;
    let audio_url = grant_url(port, &audio_token);
    let artwork_pair = match (&artwork, artwork_token.as_deref()) {
        (Some(grant), Some(artwork_token)) => {
            Some((grant_url(port, artwork_token), grant.filename.clone()))
        }
        _ => None,
    };
    let outcome = webview::do_upload(
        &app,
        &upload_id,
        &token,
        TRACKS_ENDPOINT,
        &fields,
        &audio_url,
        &audio.filename,
        artwork_pair
            .as_ref()
            .map(|(url, filename)| (url.as_str(), filename.as_str())),
        false,
    )
    .await?;

    if !(200..300).contains(&outcome.status) {
        return Err(format!(
            "アップロードに失敗しました({}): {}",
            outcome.status,
            truncate_chars(&outcome.payload, ERROR_BODY_MAX)
        ));
    }
    parse_uploaded_track(&outcome.payload)
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
        assert_eq!(soft.warnings.len(), 1);

        let at_boundary = inspect_verdict(SOFT_MAX_BYTES, Some(1_000)).unwrap();
        assert!(at_boundary.warnings.is_empty());
        assert!(inspect_verdict(HARD_MAX_BYTES, Some(1_000)).is_ok());
        assert!(inspect_verdict(HARD_MAX_BYTES + 1, Some(1_000)).is_err());
    }

    #[test]
    fn duration_thresholds() {
        let fine = inspect_verdict(1_000, Some(3_600_000)).unwrap();
        assert!(!fine.too_long && fine.warnings.is_empty());

        let boundary = inspect_verdict(1_000, Some(MAX_DURATION_MS)).unwrap();
        assert!(!boundary.too_long);

        let long = inspect_verdict(1_000, Some(MAX_DURATION_MS + 1)).unwrap();
        assert!(long.too_long);
        assert_eq!(long.warnings.len(), 1);

        let unknown = inspect_verdict(1_000, None).unwrap();
        assert!(!unknown.too_long);
        assert_eq!(unknown.warnings.len(), 1);
    }

    #[test]
    fn meta_fields_requires_title() {
        assert!(meta_fields(&meta(json!({"title": ""}))).is_err());
        assert!(meta_fields(&meta(json!({"title": "   "}))).is_err());
        let fields = meta_fields(&meta(json!({"title": "Track"}))).unwrap();
        assert_eq!(fields[0], ("track[title]".to_string(), "Track".to_string()));
    }

    #[test]
    fn meta_fields_skips_absent_and_empty_optionals() {
        let fields = meta_fields(&meta(json!({
            "title": "T",
            "artist": "",
            "genre": null,
            "description": null
        })))
        .unwrap();
        let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            vec!["track[title]", "track[downloadable]", "track[commentable]"]
        );
    }

    #[test]
    fn meta_fields_includes_provided_values() {
        let fields = meta_fields(&meta(json!({
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
        for expected in [
            ("track[artist]", "A"),
            ("track[genre]", "Techno"),
            ("track[tag_list]", "a,b"),
            ("track[description]", "d"),
            ("track[sharing]", "private"),
            ("track[embeddable_by]", "me"),
            ("track[permalink]", "my-track"),
            ("track[release_date]", "2026-10-19"),
        ] {
            assert!(
                fields.contains(&(expected.0.to_string(), expected.1.to_string())),
                "missing {}",
                expected.0
            );
        }
    }

    #[test]
    fn meta_fields_defaults_flags_to_true() {
        let fields = meta_fields(&meta(json!({"title": "T"}))).unwrap();
        assert!(fields.contains(&("track[downloadable]".to_string(), "true".to_string())));
        assert!(fields.contains(&("track[commentable]".to_string(), "true".to_string())));

        let off = meta_fields(&meta(json!({
            "title": "T",
            "downloadable": false,
            "commentable": false
        })))
        .unwrap();
        assert!(off.contains(&("track[downloadable]".to_string(), "false".to_string())));
        assert!(off.contains(&("track[commentable]".to_string(), "false".to_string())));
    }

    #[test]
    fn meta_fields_rejects_bad_sharing() {
        assert!(meta_fields(&meta(json!({"title": "T", "sharing": "unlisted"}))).is_err());
        // The empty string is the omitted-value convention, not an error.
        assert!(meta_fields(&meta(json!({"title": "T", "sharing": ""}))).is_ok());
    }

    #[test]
    fn meta_fields_accepts_camel_case_keys() {
        let fields = meta_fields(&meta(json!({
            "title": "T",
            "tagList": "a,b",
            "embeddableBy": "me",
            "releaseDate": "2026-10-19"
        })))
        .unwrap();
        assert!(fields.contains(&("track[tag_list]".to_string(), "a,b".to_string())));
        assert!(fields.contains(&("track[embeddable_by]".to_string(), "me".to_string())));
        assert!(fields.contains(&("track[release_date]".to_string(), "2026-10-19".to_string())));
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
            "日本語",
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
