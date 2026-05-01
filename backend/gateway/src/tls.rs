use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use arc_swap::ArcSwap;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::ServerConfig;

pub type HttpChallengeMap = Arc<RwLock<HashMap<String, String>>>;
pub type AlpnChallengeMap = Arc<RwLock<HashMap<String, Arc<CertifiedKey>>>>;

#[derive(Clone)]
pub struct TlsState {
    pub config: Arc<ArcSwap<ServerConfig>>,
    pub http_challenges: HttpChallengeMap,
    pub alpn_challenges: AlpnChallengeMap,
}

impl TlsState {
    pub async fn from_disk(cert: &Path, key: &Path) -> Result<Self, BuildError> {
        let server_cfg = build_server_config(cert, key).await?;
        Ok(Self {
            config: Arc::new(ArcSwap::new(Arc::new(server_cfg))),
            http_challenges: Arc::new(RwLock::new(HashMap::new())),
            alpn_challenges: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn reload(&self, cert: &Path, key: &Path) -> Result<(), BuildError> {
        let server_cfg = build_server_config(cert, key).await?;
        self.config.store(Arc::new(server_cfg));
        Ok(())
    }
}

pub fn alpn_resolver(map: AlpnChallengeMap) -> Arc<AlpnResolver> {
    Arc::new(AlpnResolver { challenges: map })
}

pub struct AlpnResolver {
    challenges: AlpnChallengeMap,
}

impl std::fmt::Debug for AlpnResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlpnResolver").finish()
    }
}

impl ResolvesServerCert for AlpnResolver {
    fn resolve(&self, hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let server_name = hello.server_name()?;
        let map = self.challenges.read().ok()?;
        map.get(server_name).cloned()
    }
}

#[derive(Debug)]
pub enum BuildError {
    Io(std::io::Error),
    Pem(String),
    Rustls(rustls::Error),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Pem(e) => write!(f, "pem: {e}"),
            Self::Rustls(e) => write!(f, "rustls: {e}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rustls::Error> for BuildError {
    fn from(e: rustls::Error) -> Self {
        Self::Rustls(e)
    }
}

async fn build_server_config(cert: &Path, key: &Path) -> Result<ServerConfig, BuildError> {
    let cert_bytes = tokio::fs::read(cert).await?;
    let key_bytes = tokio::fs::read(key).await?;

    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_bytes)
        .collect::<Result<_, _>>()
        .map_err(|e| BuildError::Pem(format!("cert: {e}")))?;
    if chain.is_empty() {
        return Err(BuildError::Pem("cert chain empty".into()));
    }

    let private_key = PrivateKeyDer::from_pem_slice(&key_bytes)
        .map_err(|e| BuildError::Pem(format!("key: {e}")))?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, private_key)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}
