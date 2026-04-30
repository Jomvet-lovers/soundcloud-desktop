//! ACME issuer with HTTP-01 + TLS-ALPN-01 fallback.
//!
//! Both listeners (`:80` and `:443`) come up at the same time so LE multi-
//! perspective probes hit something regardless of which port is unblocked
//! by the host's network. We deliberately tell ACME _which_ challenge to
//! validate (`set_challenge_ready`) so we control which path is exercised.
//!
//! Strategy:
//!   attempt 1: HTTP-01 (`:80`)
//!   attempt 2: TLS-ALPN-01 (`:443`) if HTTP-01 failed
//!
//! Hard cap of 2 attempts per session keeps us under the LE limit of
//! 5 failed authorizations / hour / hostname / account.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::Path, response::IntoResponse, routing::get, Router};
use instant_acme::{
    Account, AccountCredentials, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
    OrderStatus,
};
use rcgen::{CertificateParams, CustomExtension, DistinguishedName, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::{Acceptor, ResolvesServerCert, ServerConfig};
use rustls::sign::CertifiedKey;
use sha2::{Digest, Sha256};
use std::sync::RwLock;
use tokio::net::TcpListener;
use tokio_rustls::LazyConfigAcceptor;
use tracing::{debug, error, info, warn};

use crate::TlsConfig;

const ACME_TLS_ALPN: &[u8] = b"acme-tls/1";
const ACME_IDENTIFIER_OID: &[u64] = &[1, 3, 6, 1, 5, 5, 7, 1, 31];
const ORDER_POLL_INTERVAL: Duration = Duration::from_secs(2);
const ORDER_POLL_TIMEOUT: Duration = Duration::from_secs(90);

/// HTTP-01 challenge map: token → key authorization.
type HttpChallenges = Arc<RwLock<HashMap<String, String>>>;

/// TLS-ALPN-01 challenge map: domain → challenge cert+key.
type AlpnChallenges = Arc<RwLock<HashMap<String, Arc<CertifiedKey>>>>;

pub async fn issue(cfg: &TlsConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let account = load_or_create_account(cfg).await?;

    // Bring both listeners up — they stay alive across attempts so
    // we don't take TCP latency hits between fallback rounds.
    let http_map: HttpChallenges = Arc::new(RwLock::new(HashMap::new()));
    let alpn_map: AlpnChallenges = Arc::new(RwLock::new(HashMap::new()));
    let http_handle = spawn_http_listener(cfg.http_port, http_map.clone()).await?;
    let alpn_handle = spawn_alpn_listener(cfg.https_port, alpn_map.clone()).await?;

    let attempts = [
        (ChallengeType::Http01, "http-01"),
        (ChallengeType::TlsAlpn01, "tls-alpn-01"),
    ];

    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    let mut cert_pem: Option<String> = None;
    let mut key_pem: Option<String> = None;

    for (idx, (ctype, label)) in attempts.iter().enumerate() {
        if idx > 0 {
            // Cool-off between attempts. Avoids slamming LE if the first
            // attempt already cost us 1 of the 5 failed-authz budget.
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
        info!("[acme] attempt {} via {}", idx + 1, label);
        match try_issue(cfg, &account, ctype.clone(), &http_map, &alpn_map).await {
            Ok((c, k)) => {
                cert_pem = Some(c);
                key_pem = Some(k);
                info!("[acme] {} succeeded", label);
                break;
            }
            Err(e) => {
                warn!("[acme] {} failed: {e}", label);
                last_err = Some(e);
            }
        }
    }

    http_handle.abort();
    alpn_handle.abort();

    let (Some(cert_pem), Some(key_pem)) = (cert_pem, key_pem) else {
        return Err(last_err.unwrap_or_else(|| "ACME issue failed".into()));
    };

    write_pem(&cfg.cert_path(), cert_pem.as_bytes()).await?;
    write_pem(&cfg.key_path(), key_pem.as_bytes()).await?;

    Ok(())
}

async fn try_issue(
    cfg: &TlsConfig,
    account: &Account,
    ctype: ChallengeType,
    http_map: &HttpChallenges,
    alpn_map: &AlpnChallenges,
) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let identifiers: Vec<Identifier> = cfg
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

    // Each authorization: pick the requested challenge type, materialize the
    // response, install it in the right map.
    let mut challenge_urls = Vec::with_capacity(authorizations.len());
    for authz in &authorizations {
        let domain = match &authz.identifier {
            Identifier::Dns(d) => d.clone(),
        };

        let challenge = authz
            .challenges
            .iter()
            .find(|c| c.r#type == ctype)
            .ok_or_else(|| format!("no {:?} challenge for {domain}", ctype))?;

        let key_auth = order.key_authorization(challenge);

        match ctype {
            ChallengeType::Http01 => {
                http_map
                    .write()
                    .unwrap()
                    .insert(challenge.token.clone(), key_auth.as_str().to_string());
                debug!("[acme] http-01 token armed for {domain}");
            }
            ChallengeType::TlsAlpn01 => {
                let ck = build_alpn_challenge_cert(&domain, key_auth.digest().as_ref())?;
                alpn_map.write().unwrap().insert(domain.clone(), Arc::new(ck));
                debug!("[acme] tls-alpn-01 cert armed for {domain}");
            }
            _ => return Err("unsupported challenge type".into()),
        }

        challenge_urls.push(challenge.url.clone());
    }

    // Notify ACME we're ready to be probed.
    for url in &challenge_urls {
        order.set_challenge_ready(url).await?;
    }

    // Poll order state until valid/invalid/timeout.
    let deadline = tokio::time::Instant::now() + ORDER_POLL_TIMEOUT;
    let final_state = loop {
        if tokio::time::Instant::now() >= deadline {
            return Err("order poll timed out".into());
        }
        tokio::time::sleep(ORDER_POLL_INTERVAL).await;
        let state = order.refresh().await?;
        debug!("[acme] order state: {:?}", state.status);
        match state.status {
            OrderStatus::Pending | OrderStatus::Processing => continue,
            OrderStatus::Ready => break state,
            OrderStatus::Valid => break state,
            OrderStatus::Invalid => return Err("order became invalid".into()),
        }
    };

    // Generate keypair + CSR for finalize.
    let key_pair = KeyPair::generate()?;
    let mut params = CertificateParams::new(cfg.domains.clone())?;
    params.distinguished_name = DistinguishedName::new();
    let csr = params.serialize_request(&key_pair)?;

    if final_state.status == OrderStatus::Ready {
        order.finalize(csr.der()).await?;
    }

    // Wait for the cert chain to materialize on the order.
    let chain_pem = loop {
        if let Some(c) = order.certificate().await? {
            break c;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };

    let key_pem = key_pair.serialize_pem();

    // Cleanup challenge state for this attempt.
    if matches!(ctype, ChallengeType::Http01) {
        http_map.write().unwrap().clear();
    } else {
        alpn_map.write().unwrap().clear();
    }

    Ok((chain_pem, key_pem))
}

async fn load_or_create_account(
    cfg: &TlsConfig,
) -> Result<Account, Box<dyn std::error::Error + Send + Sync>> {
    let path = cfg.cache_dir.join("account.json");
    let url = if cfg.staging {
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

    let contact = format!("mailto:{}", cfg.email);
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

async fn write_pem(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    tokio::fs::write(path, content).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = tokio::fs::metadata(path).await?.permissions();
        perm.set_mode(0o600);
        tokio::fs::set_permissions(path, perm).await?;
    }
    Ok(())
}

/* ── HTTP-01 listener ─────────────────────────────────────────────────── */

async fn spawn_http_listener(
    port: u16,
    challenges: HttpChallenges,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    let app = Router::new()
        .route(
            "/.well-known/acme-challenge/{token}",
            get(http_challenge_handler),
        )
        .with_state(challenges);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("[acme] http-01 listener on :{port}");

    Ok(tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("http-01 listener: {e}");
        }
    }))
}

async fn http_challenge_handler(
    Path(token): Path<String>,
    axum::extract::State(challenges): axum::extract::State<HttpChallenges>,
) -> impl IntoResponse {
    let map = challenges.read().unwrap();
    if let Some(key_auth) = map.get(&token) {
        info!("[acme] http-01 hit token={}…", &token[..token.len().min(12)]);
        (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            key_auth.clone(),
        )
            .into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "").into_response()
    }
}

/* ── TLS-ALPN-01 listener ─────────────────────────────────────────────── */

async fn spawn_alpn_listener(
    port: u16,
    challenges: AlpnChallenges,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("[acme] tls-alpn-01 listener on :{port}");

    let resolver = Arc::new(AlpnResolver {
        challenges: challenges.clone(),
    });

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
                    // Not an ACME probe. Drop politely; backend will own
                    // :443 once we exit.
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

#[derive(Debug)]
struct AlpnResolver {
    challenges: AlpnChallenges,
}

impl ResolvesServerCert for AlpnResolver {
    fn resolve(&self, hello: rustls::server::ClientHello) -> Option<Arc<CertifiedKey>> {
        let server_name = hello.server_name()?;
        let map = self.challenges.read().ok()?;
        let ck = map.get(server_name)?.clone();
        info!("[acme] tls-alpn-01 hit servername={server_name}");
        Some(ck)
    }
}

fn build_alpn_challenge_cert(
    domain: &str,
    key_auth_digest: &[u8],
) -> Result<CertifiedKey, Box<dyn std::error::Error + Send + Sync>> {
    // RFC 8737: extension value is OCTET STRING(SHA-256(keyAuthorization)).
    // instant-acme already gives us the SHA-256 via key_auth.digest().
    let digest = sha256(key_auth_digest);
    let mut der_value = Vec::with_capacity(34);
    der_value.push(0x04); // OCTET STRING
    der_value.push(0x20); // length 32
    der_value.extend_from_slice(&digest);

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

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}
