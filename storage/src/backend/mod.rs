use std::path::Path;
use std::pin::Pin;

use bytes::Bytes;
use futures::Stream;

pub mod local;
pub mod s3;

pub use local::LocalBackend;
pub use s3::S3Backend;

pub type ByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static>>;

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
    Local(LocalBackend),
    S3(S3Backend),
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
        quality: &str,
    ) -> Result<(), BackendError> {
        match self {
            Backend::Local(b) => {
                b.commit_transcode(key, src_tmp, ffprobe_bin, filename, quality)
                    .await
            }
            Backend::S3(b) => b.put_file(key, src_tmp).await,
        }
    }

    pub async fn delete_file(&self, key: &str) -> Result<bool, BackendError> {
        match self {
            Backend::Local(b) => b.delete_file(key).await,
            Backend::S3(b) => b.delete_file(key).await,
        }
    }

    pub async fn head(&self, key: &str) -> Result<Option<ObjectInfo>, BackendError> {
        match self {
            Backend::Local(b) => b.head(key).await,
            Backend::S3(b) => b.head(key).await,
        }
    }

    pub async fn stream(
        &self,
        key: &str,
    ) -> Result<(ObjectInfo, ByteStream), BackendError> {
        match self {
            Backend::Local(b) => b.stream(key).await,
            Backend::S3(b) => b.stream(key).await,
        }
    }
}

pub fn key_for(quality: &str, filename: &str) -> String {
    format!("{quality}/{filename}.ogg")
}

pub fn content_type_for(key: &str) -> &'static str {
    if key.ends_with(".ogg") {
        "audio/ogg"
    } else if key.ends_with(".mp3") {
        "audio/mpeg"
    } else {
        "application/octet-stream"
    }
}
