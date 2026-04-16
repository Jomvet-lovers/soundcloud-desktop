use std::path::{Path, PathBuf};

use futures::StreamExt;
use tokio_util::io::ReaderStream;
use tracing::info;
use uuid::Uuid;

use super::{BackendError, ByteStream, ObjectInfo};

const DURATION_EPSILON_SECS: f64 = 2.0;

pub struct LocalBackend {
    root: PathBuf,
}

impl LocalBackend {
    pub async fn new(root: &str) -> Result<Self, BackendError> {
        tokio::fs::create_dir_all(root).await?;
        tokio::fs::create_dir_all(PathBuf::from(root).join("hq")).await?;
        tokio::fs::create_dir_all(PathBuf::from(root).join("sq")).await?;
        Ok(Self {
            root: PathBuf::from(root),
        })
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }

    pub async fn commit_transcode(
        &self,
        key: &str,
        src_tmp: &Path,
        ffprobe_bin: &str,
        filename: &str,
        quality: &str,
    ) -> Result<(), BackendError> {
        let dst = self.path_for(key);

        if should_keep_existing(&dst, src_tmp, quality, ffprobe_bin).await {
            let _ = tokio::fs::remove_file(src_tmp).await;
            return Ok(());
        }

        let dst_dir = dst.parent().ok_or_else(|| {
            BackendError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("destination has no parent: {}", dst.display()),
            ))
        })?;
        tokio::fs::create_dir_all(dst_dir).await?;

        let stage_path = dst_dir.join(format!(".{filename}.{quality}.{}.tmp", Uuid::new_v4()));

        if let Err(err) = move_or_copy_file(src_tmp, &stage_path).await {
            let _ = tokio::fs::remove_file(&stage_path).await;
            return Err(err);
        }

        if let Err(err) = replace_file(&stage_path, &dst).await {
            let _ = tokio::fs::remove_file(&stage_path).await;
            return Err(err);
        }

        Ok(())
    }

    pub async fn delete_file(&self, key: &str) -> Result<bool, BackendError> {
        let path = self.path_for(key);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(BackendError::Io(e)),
        }
    }

    pub async fn head(&self, key: &str) -> Result<Option<ObjectInfo>, BackendError> {
        let path = self.path_for(key);
        match tokio::fs::metadata(&path).await {
            Ok(meta) => Ok(Some(ObjectInfo {
                size: meta.len(),
                content_type: Some(super::content_type_for(key).to_string()),
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BackendError::Io(e)),
        }
    }

    pub async fn stream(
        &self,
        key: &str,
    ) -> Result<(ObjectInfo, ByteStream), BackendError> {
        let path = self.path_for(key);
        let file = match tokio::fs::File::open(&path).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(BackendError::NotFound);
            }
            Err(e) => return Err(BackendError::Io(e)),
        };
        let meta = file.metadata().await?;
        let info = ObjectInfo {
            size: meta.len(),
            content_type: Some(super::content_type_for(key).to_string()),
        };
        let stream = ReaderStream::new(file)
            .map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
        Ok((info, Box::pin(stream)))
    }
}

async fn probe_duration(path: &Path, ffprobe_bin: &str) -> Option<f64> {
    let output = tokio::process::Command::new(ffprobe_bin)
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

async fn should_keep_existing(
    dst: &Path,
    src_tmp: &Path,
    quality: &str,
    ffprobe_bin: &str,
) -> bool {
    if tokio::fs::metadata(dst).await.is_err() {
        return false;
    }
    let Some(existing) = probe_duration(dst, ffprobe_bin).await else {
        return false;
    };
    let Some(candidate) = probe_duration(src_tmp, ffprobe_bin).await else {
        return false;
    };
    if existing + DURATION_EPSILON_SECS >= candidate {
        info!(
            "[local] keeping existing {quality} {:.3}s >= new {:.3}s",
            existing, candidate
        );
        return true;
    }
    false
}

async fn move_or_copy_file(src: &Path, dst: &Path) -> Result<(), BackendError> {
    match tokio::fs::rename(src, dst).await {
        Ok(()) => Ok(()),
        Err(err) if err.raw_os_error() == Some(18) => {
            tokio::fs::copy(src, dst).await?;
            tokio::fs::remove_file(src).await?;
            Ok(())
        }
        Err(err) => Err(BackendError::Io(err)),
    }
}

async fn replace_file(src: &Path, dst: &Path) -> Result<(), BackendError> {
    match tokio::fs::rename(src, dst).await {
        Ok(()) => Ok(()),
        Err(first_err) => {
            if tokio::fs::metadata(dst).await.is_ok() {
                tokio::fs::remove_file(dst).await?;
                tokio::fs::rename(src, dst).await?;
                Ok(())
            } else {
                Err(BackendError::Io(first_err))
            }
        }
    }
}
