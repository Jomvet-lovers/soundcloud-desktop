use std::path::Path;
use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream as AwsByteStream;
use aws_sdk_s3::Client;
use tokio_util::io::ReaderStream;
use tracing::warn;

use super::{BackendError, ByteStream, ObjectInfo};
use crate::config::S3Config;

pub struct S3Backend {
    client: Client,
    bucket: String,
}

impl S3Backend {
    pub async fn new(cfg: &S3Config) -> Self {
        let creds = Credentials::new(
            &cfg.access_key_id,
            &cfg.secret_access_key,
            None,
            None,
            "storage-env",
        );

        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(Region::new(cfg.region.clone()))
            .credentials_provider(creds);

        if let Some(endpoint) = &cfg.endpoint {
            loader = loader.endpoint_url(endpoint.clone());
        }

        let shared = loader.load().await;

        let mut s3_builder = aws_sdk_s3::config::Builder::from(&shared);
        if cfg.force_path_style {
            s3_builder = s3_builder.force_path_style(true);
        }
        let client = Client::from_conf(s3_builder.build());

        Self {
            client,
            bucket: cfg.bucket.clone(),
        }
    }

    pub async fn put_file(&self, key: &str, src: &Path) -> Result<(), BackendError> {
        let body = AwsByteStream::from_path(src)
            .await
            .map_err(|e| BackendError::Other(format!("read tmp: {e}")))?;

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .content_type(super::content_type_for(key))
            .send()
            .await
            .map_err(|e| BackendError::Other(format!("put_object {key}: {e}")))?;

        let _ = tokio::fs::remove_file(src).await;
        Ok(())
    }

    pub async fn delete_file(&self, key: &str) -> Result<bool, BackendError> {
        // Head first to report whether anything was actually deleted.
        let existed = self.head(key).await?.is_some();
        if !existed {
            return Ok(false);
        }
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| BackendError::Other(format!("delete_object {key}: {e}")))?;
        Ok(true)
    }

    pub async fn head(&self, key: &str) -> Result<Option<ObjectInfo>, BackendError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => Ok(Some(ObjectInfo {
                size: out.content_length().unwrap_or(0) as u64,
                content_type: out
                    .content_type()
                    .map(|s| s.to_string())
                    .or_else(|| Some(super::content_type_for(key).to_string())),
            })),
            Err(err) => {
                let service_err = err.into_service_error();
                if service_err.is_not_found() {
                    Ok(None)
                } else {
                    warn!("[s3] head_object {key} failed: {service_err}");
                    Err(BackendError::Other(format!("head_object {key}: {service_err}")))
                }
            }
        }
    }

    pub async fn stream(
        &self,
        key: &str,
    ) -> Result<(ObjectInfo, ByteStream), BackendError> {
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|err| {
                let service_err = err.into_service_error();
                if service_err.is_no_such_key() {
                    BackendError::NotFound
                } else {
                    BackendError::Other(format!("get_object {key}: {service_err}"))
                }
            })?;

        let size = out.content_length().unwrap_or(0) as u64;
        let content_type = out
            .content_type()
            .map(|s| s.to_string())
            .or_else(|| Some(super::content_type_for(key).to_string()));

        let reader = out.body.into_async_read();
        let stream = ReaderStream::new(reader);

        Ok((ObjectInfo { size, content_type }, Box::pin(stream)))
    }

    pub async fn presign_get(&self, key: &str, expires: Duration) -> Result<String, BackendError> {
        let cfg = PresigningConfig::expires_in(expires)
            .map_err(|e| BackendError::Other(format!("presign config: {e}")))?;
        let req = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|e| BackendError::Other(format!("presign get_object {key}: {e}")))?;
        Ok(req.uri().to_string())
    }
}
