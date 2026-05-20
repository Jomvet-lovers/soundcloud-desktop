use bytes::Bytes;
use futures::future::BoxFuture;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("disabled")]
    Disabled,
    #[error("fetch: {0}")]
    Fetch(String),
}

impl Error {
    pub fn is_disabled(&self) -> bool {
        matches!(self, Error::Disabled)
    }
}

pub trait Fetcher: Send + Sync {
    fn get(
        &self,
        url: String,
        headers: Vec<(String, String)>,
    ) -> BoxFuture<'static, Result<Bytes, Error>>;

    fn post(
        &self,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Bytes, Error>>;
}

/// Всё, что нужно клиенту, чтобы самостоятельно собрать восстановленный
/// поток: переписанный init-сегмент, URL'ы сегментов и ключ.
/// Сервер не качает сами сегменты — только manifest, init и license challenge.
pub struct ClientPrep {
    pub content_type: String,
    pub init: Bytes,
    pub segment_urls: Vec<String>,
    pub key: [u8; 16],
}

pub struct Engine {}

impl Engine {
    pub fn load(_path: &Path) -> Result<Self, Error> {
        Err(Error::Disabled)
    }

    pub fn devices(&self) -> usize {
        0
    }

    pub async fn process(
        &self,
        _manifest: &str,
        _token: &str,
        _fetcher: &dyn Fetcher,
    ) -> Result<Bytes, Error> {
        Err(Error::Disabled)
    }

    pub async fn process_stream(
        &self,
        _manifest: &str,
        _token: &str,
        _fetcher: Arc<dyn Fetcher>,
    ) -> Result<futures::stream::BoxStream<'static, Result<Bytes, Error>>, Error> {
        Err(Error::Disabled)
    }

    pub async fn prepare_for_client(
        &self,
        _manifest: &str,
        _token: &str,
        _fetcher: Arc<dyn Fetcher>,
    ) -> Result<ClientPrep, Error> {
        Err(Error::Disabled)
    }
}
