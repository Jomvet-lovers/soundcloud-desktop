use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use instant_acme::{
    Account, AccountCredentials, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
    OrderStatus,
};
use rcgen::{CertificateParams, CustomExtension, DistinguishedName, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{Acceptor, ServerConfig};
use rustls::sign::CertifiedKey;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::LazyConfigAcceptor;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::tls::{alpn_resolver, AlpnChallengeMap, HttpChallengeMap};

const ACME_TLS_ALPN: &[u8] = b"acme-tls/1";
const ACME_IDENTIFIER_OID: &[u64] = &[1, 3, 6, 1, 5, 5, 7, 1, 31];
const ORDER_POLL_INTERVAL: Duration = Duration::from_secs(2);
const ORDER_POLL_TIMEOUT: Duration = Duration::from_secs(120);

type Maps = (HttpChallengeMap, AlpnChallengeMap);

pub async fn issue(
    cfg: &Config,
    inflight_maps: Option<Maps>,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let account = load_or_create_account(cfg).await?;

    let (http_map, alpn_map, _own_handles) = match inflight_maps {
        Some((h, a)) => (h, a, None),
        None => {
            let h: HttpChallengeMap = Arc::new(RwLock::new(HashMap::new()));
            let a: AlpnChallengeMap = Arc::new(RwLock::new(HashMap::new()));
            let h_handle = spawn_bootstrap_http_listener(cfg.http_port, h.clone()).await?;
            let a_handle = spawn_bootstrap_alpn_listener(cfg.https_port, a.clone()).await?;
            (h, a, Some(BootstrapHandles(h_handle, a_handle)))
        }
    };

    let attempts = [
        (ChallengeType::Http01, "http-01"),
        (ChallengeType::TlsAlpn01, "tls-alpn-01"),
    ];

    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    let mut result = None;

    for (idx, (ctype, label)) in attempts.iter().enumerate() {
        if idx > 0 {
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
        info!("[acme] attempt {} via {}", idx + 1, label);
        match try_issue(cfg, &account, ctype.clone(), &http_map, &alpn_map).await {
            Ok(pair) => {
                info!("[acme] {label} succeeded");
                result = Some(pair);
                break;
            }
            Err(e) => {
                warn!("[acme] {label} failed: {e}");
                last_err = Some(e);
            }
        }
    }

    result.ok_or_else(|| last_err.unwrap_or_else(|| "ACME issue failed".into()))
}

struct BootstrapHandles(JoinHandle<()>, JoinHandle<()>);
impl Drop for BootstrapHandles {
    fn drop(&mut self) {
        self.0.abort();
        self.1.abort();
    }
}

async fn try_issue(
    cfg: &Config,
    account: &Account,
    ctype: ChallengeType,
    http_map: &HttpChallengeMap,
    alpn_map: &AlpnChallengeMap,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let identifiers: Vec<Identifier> = cfg
        .tls
        .domains
        .iter()
        .map(|d| Identifier::Dns(d.clone()))
        .collect();

    let mut order = account
        .new_order(&NewOrder {
            identifiers: &identifiers,
        })
        .await?;

    let authorizations = order.authorizations().await?;
    let mut challenge_urls = Vec::with_capacity(authorizations.len());

    for authz in &authorizations {
        let domain = match &authz.identifier {
            Identifier::Dns(d) => d.clone(),
        };
        let challenge = authz
            .challenges
            .iter()
            .find(|c| c.r#type == ctype)
            .ok_or_else(|| format!("no {ctype:?} challenge for {domain}"))?;
        let key_auth = order.key_authorization(challenge);

        match ctype {
            ChallengeType::Http01 => {
                http_map
                    .write()
                    .unwrap()
                    .insert(challenge.token.clone(), key_auth.as_str().to_string());
            }
            ChallengeType::TlsAlpn01 => {
                let ck = build_alpn_challenge_cert(&domain, key_auth.digest().as_ref())?;
                alpn_map
                    .write()
                    .unwrap()
                    .insert(domain.clone(), Arc::new(ck));
            }
            _ => return Err("unsupported challenge type".into()),
        }
        challenge_urls.push(challenge.url.clone());
    }

    for url in &challenge_urls {
        order.set_challenge_ready(url).await?;
    }

    let deadline = tokio::time::Instant::now() + ORDER_POLL_TIMEOUT;
    let final_state = loop {
        if tokio::time::Instant::now() >= deadline {
            cleanup(ctype.clone(), http_map, alpn_map);
            return Err("order poll timed out".into());
        }
        tokio::time::sleep(ORDER_POLL_INTERVAL).await;
        let state = order.refresh().await?;
        debug!("[acme] order: {:?}", state.status);
        match state.status {
            OrderStatus::Pending | OrderStatus::Processing => continue,
            OrderStatus::Ready | OrderStatus::Valid => break state,
            OrderStatus::Invalid => {
                cleanup(ctype.clone(), http_map, alpn_map);
                return Err("order became invalid".into());
            }
        }
    };

    let key_pair = KeyPair::generate()?;
    let mut params = CertificateParams::new(cfg.tls.domains.clone())?;
    params.distinguished_name = DistinguishedName::new();
    let csr = params.serialize_request(&key_pair)?;

    if final_state.status == OrderStatus::Ready {
        order.finalize(csr.der()).await?;
    }

    let chain_pem = loop {
        if let Some(c) = order.certificate().await? {
            break c;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };

    let key_pem = key_pair.serialize_pem();
    cleanup(ctype, http_map, alpn_map);
    Ok((chain_pem, key_pem))
}

fn cleanup(ctype: ChallengeType, http_map: &HttpChallengeMap, alpn_map: &AlpnChallengeMap) {
    match ctype {
        ChallengeType::Http01 => http_map.write().unwrap().clear(),
        ChallengeType::TlsAlpn01 => alpn_map.write().unwrap().clear(),
        _ => {}
    }
}

async fn load_or_create_account(
    cfg: &Config,
) -> Result<Account, Box<dyn std::error::Error + Send + Sync>> {
    let path = cfg.tls.cache_dir.join("account.json");
    let url = if cfg.tls.staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };

    if let Ok(bytes) = tokio::fs::read(&path).await {
        if let Ok(creds) = serde_json::from_slice::<AccountCredentials>(&bytes) {
            info!("[acme] loaded existing account");
            return Ok(Account::from_credentials(creds).await?);
        }
        warn!("[acme] account file unparseable, creating new");
    }

    let contact = format!("mailto:{}", cfg.tls.email);
    let (account, creds) = Account::create(
        &NewAccount {
            contact: &[&contact],
            terms_of_service_agreed: true,
            only_return_existing: false,
        },
        url,
        None,
    )
    .await?;
    tokio::fs::write(&path, serde_json::to_vec(&creds)?).await?;
    info!("[acme] new account registered");
    Ok(account)
}

async fn spawn_bootstrap_http_listener(
    port: u16,
    challenges: HttpChallengeMap,
) -> std::io::Result<JoinHandle<()>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("[acme] bootstrap http-01 on :{port}");

    Ok(tokio::spawn(async move {
        loop {
            let (stream, _peer) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => {
                    error!("[acme] http accept: {e}");
                    continue;
                }
            };
            let challenges = challenges.clone();
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let svc = service_fn(move |req: Request<Incoming>| {
                    let challenges = challenges.clone();
                    async move { Ok::<_, hyper::Error>(handle_http01(req, challenges).await) }
                });
                let _ = http1::Builder::new().serve_connection(io, svc).await;
            });
        }
    }))
}

pub async fn handle_http01(
    req: Request<Incoming>,
    challenges: HttpChallengeMap,
) -> Response<http_body_util::combinators::BoxBody<Bytes, hyper::Error>> {
    let path = req.uri().path();
    let token = match path.strip_prefix("/.well-known/acme-challenge/") {
        Some(t) => t,
        None => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(empty_body())
                .unwrap();
        }
    };
    let body = {
        let map = challenges.read().unwrap();
        map.get(token).cloned()
    };
    match body {
        Some(key_auth) => {
            info!("[acme] http-01 hit token={}…", &token[..token.len().min(12)]);
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain")
                .body(full_body(key_auth.into_bytes()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(empty_body())
            .unwrap(),
    }
}

fn empty_body() -> http_body_util::combinators::BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full_body(data: Vec<u8>) -> http_body_util::combinators::BoxBody<Bytes, hyper::Error> {
    Full::new(Bytes::from(data))
        .map_err(|never| match never {})
        .boxed()
}

async fn spawn_bootstrap_alpn_listener(
    port: u16,
    challenges: AlpnChallengeMap,
) -> std::io::Result<JoinHandle<()>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("[acme] bootstrap tls-alpn-01 on :{port}");

    let resolver = alpn_resolver(challenges);

    Ok(tokio::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => {
                    error!("[acme] tls accept: {e}");
                    continue;
                }
            };
            let resolver = resolver.clone();
            tokio::spawn(async move {
                let acceptor = LazyConfigAcceptor::new(Acceptor::default(), stream);
                let handshake = match acceptor.await {
                    Ok(h) => h,
                    Err(e) => {
                        debug!("[acme] tls preface from {peer}: {e}");
                        return;
                    }
                };
                let client_hello = handshake.client_hello();
                let is_acme = client_hello
                    .alpn()
                    .map(|mut iter| iter.any(|p| p == ACME_TLS_ALPN))
                    .unwrap_or(false);
                if !is_acme {
                    debug!("[acme] non-acme TLS probe from {peer}");
                    return;
                }
                let mut cfg = ServerConfig::builder()
                    .with_no_client_auth()
                    .with_cert_resolver(resolver.clone());
                cfg.alpn_protocols = vec![ACME_TLS_ALPN.to_vec()];
                let _ = handshake.into_stream(Arc::new(cfg)).await;
            });
        }
    }))
}

fn build_alpn_challenge_cert(
    domain: &str,
    key_auth_digest: &[u8],
) -> Result<CertifiedKey, Box<dyn std::error::Error + Send + Sync>> {
    if key_auth_digest.len() != 32 {
        return Err(format!(
            "expected 32-byte SHA-256 digest, got {}",
            key_auth_digest.len()
        )
        .into());
    }
    let mut der_value = Vec::with_capacity(34);
    der_value.push(0x04);
    der_value.push(0x20);
    der_value.extend_from_slice(key_auth_digest);

    let key_pair = KeyPair::generate()?;
    let mut params = CertificateParams::new(vec![domain.to_string()])?;
    params.distinguished_name = DistinguishedName::new();
    let mut ext = CustomExtension::from_oid_content(ACME_IDENTIFIER_OID, der_value);
    ext.set_criticality(true);
    params.custom_extensions.push(ext);
    let cert = params.self_signed(&key_pair)?;

    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der: PrivateKeyDer<'static> =
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&key_der)?;
    Ok(CertifiedKey::new(vec![cert_der], signing_key))
}
