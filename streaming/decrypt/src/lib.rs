use bytes::Bytes;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("disabled")]
    Disabled,
}

impl Error {
    pub fn is_disabled(&self) -> bool {
        matches!(self, Error::Disabled)
    }
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
        _http: &reqwest::Client,
    ) -> Result<Bytes, Error> {
        Err(Error::Disabled)
    }

    pub async fn process_stream(
        &self,
        _manifest: &str,
        _token: &str,
        _http: &reqwest::Client,
    ) -> Result<futures::stream::BoxStream<'static, Result<Bytes, Error>>, Error> {
        Err(Error::Disabled)
    }
}
