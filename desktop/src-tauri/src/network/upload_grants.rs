//! One-time grants for serving local upload files to the hidden writer WebView.
//!
//! The writer page runs on soundcloud.com and cannot use Tauri IPC or read
//! arbitrary user files. Instead, the main window asks Rust for a token and
//! hands the writer page a URL. The bytes are resolved here from the token, so
//! file paths never leave the process.

// The issuance side is wired into Tauri commands by the upload commit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::fs;

/// SoundCloud rejects request bodies above 500 MB.
pub const MAX_BYTES: u64 = 500 * 1024 * 1024;

/// How long an issued token remains valid.
pub const TTL: Duration = Duration::from_secs(30 * 60);

const ALLOW_ORIGIN: (&str, &str) = ("access-control-allow-origin", "*");
const ALLOW_PRIVATE_NETWORK: (&str, &str) = ("access-control-allow-private-network", "true");
const EXPOSE_CONTENT_LENGTH: (&str, &str) = ("access-control-expose-headers", "content-length");

/// CORS headers shared by both one-time delivery transports (the local warp
/// server and the `scproxy://` scheme).
///
/// The writer page runs on `https://soundcloud.com`. Requests to
/// `http://127.0.0.1` therefore trigger Chrome's Private Network Access
/// preflight, which fails unless the response opts in with
/// `Access-Control-Allow-Private-Network: true`.
pub fn cors_headers() -> [(&'static str, &'static str); 3] {
    [ALLOW_ORIGIN, ALLOW_PRIVATE_NETWORK, EXPOSE_CONTENT_LENGTH]
}

/// Superset of [`cors_headers`] for preflight (`OPTIONS`) responses.
pub fn preflight_cors_headers() -> [(&'static str, &'static str); 6] {
    [
        ALLOW_ORIGIN,
        ALLOW_PRIVATE_NETWORK,
        EXPOSE_CONTENT_LENGTH,
        ("access-control-allow-methods", "GET, HEAD, OPTIONS"),
        ("access-control-allow-headers", "*"),
        ("access-control-max-age", "600"),
    ]
}

/// A one-time file delivery grant.
#[derive(Debug, Clone)]
pub struct Grant {
    pub path: PathBuf,
    pub filename: String,
    pub content_type: String,
    /// File size observed when the token was issued (0 if it could not be read).
    pub size: u64,
}

struct GrantEntry {
    grant: Grant,
    expires_at: Instant,
}

impl GrantEntry {
    fn is_expired(&self, now: Instant) -> bool {
        self.expires_at <= now
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantError {
    Unknown,
    TooLarge,
    Unreadable,
}

impl GrantError {
    pub fn status(self) -> u16 {
        match self {
            Self::Unknown => 404,
            Self::TooLarge => 413,
            Self::Unreadable => 500,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Unknown => "unknown or expired grant",
            Self::TooLarge => "file too large",
            Self::Unreadable => "failed to read file",
        }
    }
}

static GRANTS: OnceLock<Mutex<HashMap<String, GrantEntry>>> = OnceLock::new();

fn grants() -> &'static Mutex<HashMap<String, GrantEntry>> {
    GRANTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Issue a token valid for [`TTL`].
pub fn issue(path: PathBuf, filename: String, content_type: String) -> String {
    issue_with_ttl(path, filename, content_type, TTL)
}

/// Issue a token with an explicit TTL (tests use a short lifetime).
pub fn issue_with_ttl(
    path: PathBuf,
    filename: String,
    content_type: String,
    ttl: Duration,
) -> String {
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let token = uuid::Uuid::new_v4().to_string();
    let entry = GrantEntry {
        grant: Grant {
            path,
            filename,
            content_type,
            size,
        },
        expires_at: Instant::now() + ttl,
    };
    let mut map = grants().lock().unwrap_or_else(|e| e.into_inner());
    map.insert(token.clone(), entry);
    token
}

/// Consume a token. Returns `None` for unknown, expired, or already used tokens.
pub fn take(token: &str) -> Option<Grant> {
    let now = Instant::now();
    let mut map = grants().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|_, entry| !entry.is_expired(now));
    map.remove(token).map(|entry| entry.grant)
}

/// Look a token up without consuming it, so the upload command can read the
/// filename and content type before the writer page fetches the bytes.
/// Expired entries are dropped first, matching [`take`].
pub fn peek(token: &str) -> Option<Grant> {
    let now = Instant::now();
    let mut map = grants().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|_, entry| !entry.is_expired(now));
    map.get(token).map(|entry| entry.grant.clone())
}

/// Resolve and read a granted file. The token is consumed even when the read
/// fails, matching the one-time contract.
pub async fn read_grant(token: &str) -> Result<(Vec<u8>, Grant), GrantError> {
    let grant = take(token).ok_or(GrantError::Unknown)?;
    if grant.size > MAX_BYTES {
        return Err(GrantError::TooLarge);
    }
    let data = fs::read(&grant.path)
        .await
        .map_err(|_| GrantError::Unreadable)?;
    if data.len() as u64 > MAX_BYTES {
        return Err(GrantError::TooLarge);
    }
    Ok((data, grant))
}

/// MIME type from the file extension. Unknown extensions fall back to
/// `application/octet-stream`.
pub fn content_type_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp3") | Some("mp2") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("flac") => "audio/flac",
        Some("wav") => "audio/wav",
        Some("ogg") | Some("opus") => "audio/ogg",
        Some("aiff") | Some("aif") => "audio/aiff",
        Some("wma") => "audio/x-ms-wma",
        Some("amr") => "audio/amr",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::{GrantError, content_type_for, issue, issue_with_ttl, peek, read_grant, take};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn grant(path: &str, ttl: Duration) -> String {
        issue_with_ttl(
            PathBuf::from(path),
            "track.mp3".into(),
            "audio/mpeg".into(),
            ttl,
        )
    }

    #[test]
    fn token_is_single_use() {
        let token = grant("C:/uploads/track.mp3", Duration::from_secs(60));
        assert!(take(&token).is_some());
        assert!(take(&token).is_none());
    }

    #[test]
    fn expired_token_is_rejected() {
        let token = grant("C:/uploads/track.mp3", Duration::from_secs(0));
        assert!(take(&token).is_none());
    }

    #[test]
    fn unknown_token_is_rejected() {
        assert!(take("00000000-0000-4000-8000-000000000000").is_none());
    }

    #[test]
    fn peek_does_not_consume() {
        let token = grant("C:/uploads/track.mp3", Duration::from_secs(60));
        let peeked = peek(&token).expect("fresh token should peek");
        assert_eq!(peeked.filename, "track.mp3");
        // Still consumable: peek must not remove the entry.
        assert!(take(&token).is_some());
        assert!(peek(&token).is_none());
    }

    #[test]
    fn peek_rejects_expired_and_unknown() {
        let expired = grant("C:/uploads/track.mp3", Duration::from_secs(0));
        assert!(peek(&expired).is_none());
        assert!(peek("00000000-0000-4000-8000-000000000000").is_none());
    }

    #[test]
    fn content_type_follows_extension() {
        assert_eq!(content_type_for(Path::new("a.mp3")), "audio/mpeg");
        assert_eq!(content_type_for(Path::new("a.FLAC")), "audio/flac");
        assert_eq!(content_type_for(Path::new("a.jpg")), "image/jpeg");
        assert_eq!(
            content_type_for(Path::new("a.unknown")),
            "application/octet-stream"
        );
    }

    #[test]
    fn token_does_not_expose_the_file_path() {
        let path = "C:/very/secret/location/hidden-track.mp3";
        let token = issue(
            PathBuf::from(path),
            "hidden-track.mp3".into(),
            "audio/mpeg".into(),
        );
        assert!(!token.contains("secret"));
        assert!(!token.contains("hidden-track"));
        assert!(!token.contains('/'));
        assert!(!token.contains('\\'));
        assert_eq!(token.len(), 36);
    }

    #[test]
    fn granted_file_bytes_are_returned() {
        let path =
            std::env::temp_dir().join(format!("scd-upload-grant-{}.bin", uuid::Uuid::new_v4()));
        let bytes = b"fake audio bytes for the grant test";
        std::fs::write(&path, bytes).unwrap();

        let token = issue(
            path.clone(),
            "track.bin".into(),
            "application/octet-stream".into(),
        );
        let taken = take(&token).expect("token should resolve");
        assert_eq!(taken.filename, "track.bin");
        assert_eq!(taken.size, bytes.len() as u64);
        assert_eq!(std::fs::read(&taken.path).unwrap(), bytes);
        assert!(take(&token).is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn read_grant_consumes_unknown_token() {
        assert_eq!(
            read_grant("00000000-0000-4000-8000-000000000000")
                .await
                .unwrap_err(),
            GrantError::Unknown
        );
    }
}
