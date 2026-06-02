use std::path::Path;
use std::pin::Pin;

use bytes::Bytes;
use futures::Stream;

pub mod gdrive;
pub mod local;
pub mod s3;

pub use gdrive::GdriveBackend;
pub use local::LocalBackend;
pub use s3::S3Backend;

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static>>;

pub struct ObjectInfo {
    pub size: u64,
    pub content_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("not found")]
    NotFound,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("backend: {0}")]
    Other(String),
}

pub enum Backend {
    Local(Box<LocalBackend>),
    S3(Box<S3Backend>),
    Gdrive(Box<GdriveBackend>),
}

impl Backend {
    /// Commit an already-transcoded tmp file into storage under `key`.
    /// For local backend, also honors "keep existing if longer" semantics.
    pub async fn commit_transcode(
        &self,
        key: &str,
        src_tmp: &Path,
        ffprobe_bin: &str,
        filename: &str,
    ) -> Result<(), BackendError> {
        match self {
            Backend::Local(b) => {
                b.commit_transcode(key, src_tmp, ffprobe_bin, filename)
                    .await
            }
            Backend::S3(b) => b.put_file(key, src_tmp).await,
            Backend::Gdrive(b) => b.put_file(key, src_tmp).await,
        }
    }

    pub async fn delete_file(&self, key: &str) -> Result<bool, BackendError> {
        match self {
            Backend::Local(b) => b.delete_file(key).await,
            Backend::S3(b) => b.delete_file(key).await,
            Backend::Gdrive(b) => b.delete_file(key).await,
        }
    }

    pub async fn head(&self, key: &str) -> Result<Option<ObjectInfo>, BackendError> {
        match self {
            Backend::Local(b) => b.head(key).await,
            Backend::S3(b) => b.head(key).await,
            Backend::Gdrive(b) => b.head(key).await,
        }
    }

    pub async fn stream(&self, key: &str) -> Result<(ObjectInfo, ByteStream), BackendError> {
        match self {
            Backend::Local(b) => b.stream(key).await,
            Backend::S3(b) => b.stream(key).await,
            Backend::Gdrive(b) => b.stream(key).await,
        }
    }
}

pub fn key_for(filename: &str) -> String {
    format!("{filename}.m4a")
}

/// Canonical S3 object stem for a SoundCloud track: `soundcloud_tracks_<digits>`.
/// Accepts an already-canonical stem (optionally with a `.m4a` suffix) or a bare
/// numeric SC track id and coerces it to canonical. Returns `None` for anything
/// else. The `/upload` boundary rejects non-canonical names so a bare
/// `<id>.m4a` can never land in storage again.
pub fn canonical_track_filename(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".m4a").unwrap_or(name);
    if let Some(id) = stem.strip_prefix("soundcloud_tracks_") {
        return is_sc_id(id).then(|| stem.to_string());
    }
    is_sc_id(stem).then(|| format!("soundcloud_tracks_{stem}"))
}

fn is_sc_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::canonical_track_filename as canon;

    #[test]
    fn accepts_and_keeps_canonical() {
        assert_eq!(canon("soundcloud_tracks_12345").unwrap(), "soundcloud_tracks_12345");
        assert_eq!(canon("soundcloud_tracks_12345.m4a").unwrap(), "soundcloud_tracks_12345");
    }

    #[test]
    fn coerces_bare_id() {
        assert_eq!(canon("12345").unwrap(), "soundcloud_tracks_12345");
        assert_eq!(canon("12345.m4a").unwrap(), "soundcloud_tracks_12345");
    }

    #[test]
    fn rejects_non_canonical() {
        assert!(canon("soundcloud_tracks_abc").is_none());
        assert!(canon("remix_12345").is_none());
        assert!(canon("soundcloud_tracks_").is_none());
        assert!(canon("").is_none());
        assert!(canon("hello").is_none());
    }
}

pub fn content_type_for(key: &str) -> &'static str {
    if key.ends_with(".m4a") {
        "audio/mp4"
    } else if key.ends_with(".ogg") {
        "audio/ogg"
    } else if key.ends_with(".mp3") {
        "audio/mpeg"
    } else {
        "application/octet-stream"
    }
}
