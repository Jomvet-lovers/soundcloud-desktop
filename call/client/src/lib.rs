use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("call-client disabled")]
    Disabled,
}

impl Error {
    pub fn is_disabled(&self) -> bool {
        matches!(self, Error::Disabled)
    }
}

#[derive(Clone, Debug)]
pub struct Identity;

pub struct IdentityStore {
    path: PathBuf,
}

impl IdentityStore {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> Result<PathBuf, Error> {
        Err(Error::Disabled)
    }

    pub fn default_store() -> Result<Self, Error> {
        Err(Error::Disabled)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Option<Identity>, Error> {
        Err(Error::Disabled)
    }

    pub fn save(&self, _id: &Identity) -> Result<(), Error> {
        Err(Error::Disabled)
    }
}

#[derive(Clone, Debug)]
pub struct ProvisionInput {
    pub app_version: String,
    pub platform: String,
    pub pow_difficulty_bits: u32,
}

pub struct AgentConfig {
    pub endpoint_url: String,
    pub identity: Arc<Identity>,
    pub http: reqwest::Client,
    pub heartbeat_interval_ms: u64,
}

pub async fn provision(_endpoint_url: &str, _input: ProvisionInput) -> Result<Identity, Error> {
    Err(Error::Disabled)
}

pub async fn run_agent(_cfg: AgentConfig) -> Result<(), Error> {
    Err(Error::Disabled)
}
