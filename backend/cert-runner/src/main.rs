//! cert-runner — supervises a child process and keeps a TLS cert fresh.
//!
//! Workflow:
//! 1. read TlsConfig from env (same vars as tls-common)
//! 2. ensure a valid cert is on disk (issue via rustls-acme TLS-ALPN-01 on :443
//!    if missing or expiring within `RENEW_BEFORE_DAYS`)
//! 3. spawn the child with `TLS_CERT_FILE` / `TLS_KEY_FILE` env pointing at
//!    the materialized PEMs
//! 4. background-watch expiry every `RENEWAL_CHECK_INTERVAL`; when the cert
//!    is close to expiring, gracefully stop the child, re-issue, restart it
//! 5. forward SIGTERM/SIGINT to the child and propagate its exit code
//!
//! When `TLS_ENABLED` is not set, cert-runner just execs the child and acts
//! as a thin pid-1 (no ACME, no env injection).

use std::env;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use rustls_acme::caches::DirCache;
use rustls_acme::AcmeConfig;
use tokio::process::{Child, Command};
use tokio::signal::unix::{signal, SignalKind};
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

const RENEW_BEFORE_DAYS: i64 = 3;
const RENEWAL_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
const ACME_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone)]
struct TlsConfig {
    domains: Vec<String>,
    email: String,
    cache_dir: PathBuf,
    staging: bool,
    https_port: u16,
}

impl TlsConfig {
    fn from_env() -> Option<Self> {
        if !env_bool("TLS_ENABLED", false) {
            return None;
        }
        let domains = parse_csv(&env::var("DOMAINS").unwrap_or_default());
        if domains.is_empty() {
            panic!("TLS_ENABLED=true but DOMAINS is empty (expected comma-separated domain list)");
        }
        let email = env::var("ACME_EMAIL").unwrap_or_else(|_| format!("admin@{}", domains[0]));
        let cache_dir = PathBuf::from(
            env::var("ACME_CACHE_DIR").unwrap_or_else(|_| "/var/cache/acme".to_string()),
        );
        let staging = env_bool("ACME_STAGING", false);
        let https_port = env_u16("TLS_HTTPS_PORT", 443);
        Some(Self {
            domains,
            email,
            cache_dir,
            staging,
            https_port,
        })
    }

    fn cert_path(&self) -> PathBuf {
        self.cache_dir.join("cert.pem")
    }
    fn key_path(&self) -> PathBuf {
        self.cache_dir.join("key.pem")
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cert_runner=info".parse().unwrap()),
        )
        .init();

    let argv: Vec<OsString> = env::args_os().skip(1).collect();
    if argv.is_empty() {
        eprintln!("usage: cert-runner <child-cmd> [args...]");
        return ExitCode::from(2);
    }

    let cfg = TlsConfig::from_env();

    match run(cfg, &argv).await {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            error!("fatal: {e}");
            ExitCode::from(1)
        }
    }
}

async fn run(cfg: Option<TlsConfig>, argv: &[OsString]) -> std::io::Result<u8> {
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    let Some(cfg) = cfg else {
        // No TLS — exec child verbatim, no env injection.
        info!("TLS_ENABLED is not set, running child without ACME");
        let mut child = spawn_child(argv, None)?;
        return wait_with_signals(&mut child, &mut sigterm, &mut sigint).await;
    };

    info!(
        "TLS enabled for {} (cache={}, staging={})",
        cfg.domains.join(","),
        cfg.cache_dir.display(),
        cfg.staging,
    );

    loop {
        ensure_cert(&cfg).await?;
        let mut child = spawn_child(argv, Some(&cfg))?;

        let (renew_tx, mut renew_rx) = tokio::sync::oneshot::channel::<()>();
        let watcher_cfg = cfg.clone();
        let watcher = tokio::spawn(async move { renewal_watcher(watcher_cfg, renew_tx).await });

        let action = tokio::select! {
            status = child.wait() => Action::ChildExited(status?.code().unwrap_or(0) as u8),
            _ = sigterm.recv() => Action::Signal,
            _ = sigint.recv() => Action::Signal,
            res = &mut renew_rx => {
                match res {
                    Ok(()) => Action::RenewNeeded,
                    Err(_) => Action::Signal,
                }
            }
        };
        watcher.abort();

        match action {
            Action::ChildExited(code) => return Ok(code),
            Action::Signal => {
                info!("forwarding shutdown signal to child");
                let _ = child.start_kill();
                let status = child.wait().await?;
                return Ok(status.code().unwrap_or(0) as u8);
            }
            Action::RenewNeeded => {
                warn!("cert renewal triggered — stopping child to reclaim :{}", cfg.https_port);
                let _ = child.start_kill();
                let _ = child.wait().await;
                // loop → ensure_cert (will renew) → respawn child
            }
        }
    }
}

enum Action {
    ChildExited(u8),
    Signal,
    RenewNeeded,
}

async fn wait_with_signals(
    child: &mut Child,
    sigterm: &mut tokio::signal::unix::Signal,
    sigint: &mut tokio::signal::unix::Signal,
) -> std::io::Result<u8> {
    tokio::select! {
        status = child.wait() => Ok(status?.code().unwrap_or(0) as u8),
        _ = sigterm.recv() => {
            let _ = child.start_kill();
            let status = child.wait().await?;
            Ok(status.code().unwrap_or(0) as u8)
        }
        _ = sigint.recv() => {
            let _ = child.start_kill();
            let status = child.wait().await?;
            Ok(status.code().unwrap_or(0) as u8)
        }
    }
}

fn spawn_child(argv: &[OsString], cfg: Option<&TlsConfig>) -> std::io::Result<Child> {
    let (program, args) = argv.split_first().expect("argv non-empty");
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(cfg) = cfg {
        cmd.env("TLS_CERT_FILE", cfg.cert_path());
        cmd.env("TLS_KEY_FILE", cfg.key_path());
    }
    cmd.kill_on_drop(true);
    cmd.spawn()
}

async fn renewal_watcher(cfg: TlsConfig, tx: tokio::sync::oneshot::Sender<()>) {
    let cert_path = cfg.cert_path();
    loop {
        tokio::time::sleep(RENEWAL_CHECK_INTERVAL).await;
        match remaining_days(&cert_path).await {
            Ok(days) => {
                info!("[watcher] cert remaining: {days}d");
                if days <= RENEW_BEFORE_DAYS {
                    let _ = tx.send(());
                    return;
                }
            }
            Err(e) => {
                warn!("[watcher] cert read failed: {e}");
            }
        }
    }
}

async fn ensure_cert(cfg: &TlsConfig) -> std::io::Result<()> {
    tokio::fs::create_dir_all(&cfg.cache_dir).await?;
    if let Ok(days) = remaining_days(&cfg.cert_path()).await {
        if days > RENEW_BEFORE_DAYS {
            info!("cert valid for {days}d — skipping issue");
            return Ok(());
        }
        warn!("cert expires in {days}d (≤{RENEW_BEFORE_DAYS}) — renewing");
    } else {
        info!("no cert in cache — issuing fresh");
    }
    issue(cfg).await?;
    extract_pem(&cfg.cache_dir, &cfg.domains).await?;
    let days = remaining_days(&cfg.cert_path()).await.unwrap_or(0);
    info!("cert ready ({days}d remaining)");
    Ok(())
}

/// Drives rustls-acme through a single TLS-ALPN-01 issue using its DirCache.
/// Binds :https_port to satisfy the challenge, drains it on success.
async fn issue(cfg: &TlsConfig) -> std::io::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut state = AcmeConfig::new(cfg.domains.clone())
        .contact_push(format!("mailto:{}", cfg.email))
        .cache(DirCache::new(cfg.cache_dir.clone()))
        .directory_lets_encrypt(!cfg.staging)
        .state();

    let rustls_cfg = state.default_rustls_config();
    let acceptor = state.axum_acceptor(rustls_cfg);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.https_port));
    let handle = axum_server::Handle::new();
    let server_handle = handle.clone();

    let server_task = tokio::spawn(async move {
        let app = axum::Router::<()>::new();
        if let Err(e) = axum_server::bind(addr)
            .handle(server_handle)
            .acceptor(acceptor)
            .serve(app.into_make_service())
            .await
        {
            error!("acme listener error on :{}: {e}", addr.port());
        }
    });

    let pump = async {
        while let Some(evt) = state.next().await {
            match evt {
                Ok(ok) => info!("acme: {:?}", ok),
                Err(e) => error!("acme err: {:?}", e),
            }
        }
    };

    // Wait for the cert file to materialize in DirCache (rustls-acme writes it
    // synchronously once the order completes). Filename layout varies across
    // versions, so we sniff PEM content rather than guessing the hash.
    let cache_dir = cfg.cache_dir.clone();
    let waiter = async {
        loop {
            if find_dir_cache_cert(&cache_dir).await.is_ok() {
                return Ok::<(), std::io::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    };

    let result = tokio::select! {
        _ = pump => Err(std::io::Error::other("acme stream ended unexpectedly")),
        r = waiter => r,
        _ = tokio::time::sleep(ACME_TIMEOUT) => Err(std::io::Error::other("acme issue timed out")),
    };

    handle.graceful_shutdown(Some(Duration::from_secs(2)));
    let _ = server_task.await;

    result
}

/// Convert rustls-acme's combined PEM (private-key + cert-chain) into separate
/// `cert.pem` / `key.pem` consumable by Node's `https.createServer`.
async fn extract_pem(cache_dir: &Path, _domains: &[String]) -> std::io::Result<()> {
    let combined_path = find_dir_cache_cert(cache_dir).await?;
    let combined = tokio::fs::read_to_string(&combined_path).await?;

    let blocks = pem::parse_many(combined.as_bytes())
        .map_err(|e| std::io::Error::other(format!("pem parse: {e}")))?;

    let mut key_blocks = Vec::new();
    let mut cert_blocks = Vec::new();
    for blk in blocks {
        match blk.tag().to_uppercase().as_str() {
            "PRIVATE KEY" | "EC PRIVATE KEY" | "RSA PRIVATE KEY" => key_blocks.push(blk),
            "CERTIFICATE" => cert_blocks.push(blk),
            other => warn!("unexpected pem block: {other}"),
        }
    }

    if key_blocks.is_empty() {
        return Err(std::io::Error::other("no PRIVATE KEY block in cached cert"));
    }
    if cert_blocks.is_empty() {
        return Err(std::io::Error::other("no CERTIFICATE block in cached cert"));
    }

    let key_pem: String = key_blocks.iter().map(pem::encode).collect();
    let cert_pem: String = cert_blocks.iter().map(pem::encode).collect();

    let cert_path = cache_dir.join("cert.pem");
    let key_path = cache_dir.join("key.pem");
    tokio::fs::write(&cert_path, cert_pem).await?;
    tokio::fs::write(&key_path, key_pem).await?;

    // Tighten key permissions: Node reads cert.pem/key.pem at boot, only the
    // cert-runner / backend should be able to.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = tokio::fs::metadata(&key_path).await?.permissions();
        perm.set_mode(0o600);
        tokio::fs::set_permissions(&key_path, perm).await?;
    }

    Ok(())
}

async fn find_dir_cache_cert(cache_dir: &Path) -> std::io::Result<PathBuf> {
    let mut entries = tokio::fs::read_dir(cache_dir).await?;
    let mut candidate: Option<PathBuf> = None;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Skip our own derived files and anything obviously not a cached cert.
        if name == "cert.pem" || name == "key.pem" || name.ends_with(".account") {
            continue;
        }
        // rustls-acme writes the cert file with no specific extension;
        // disambiguate by sniffing PEM content.
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            if content.contains("-----BEGIN CERTIFICATE-----")
                && content.contains("-----BEGIN ")
                && content.contains("PRIVATE KEY-----")
            {
                candidate = Some(path);
                break;
            }
        }
    }
    candidate.ok_or_else(|| std::io::Error::other("no cached cert file found in ACME_CACHE_DIR"))
}

async fn remaining_days(cert_path: &Path) -> std::io::Result<i64> {
    let pem_bytes = tokio::fs::read(cert_path).await?;
    let (_, parsed) = x509_parser::pem::parse_x509_pem(&pem_bytes)
        .map_err(|e| std::io::Error::other(format!("pem: {e}")))?;
    let (_, cert) = x509_parser::parse_x509_certificate(&parsed.contents)
        .map_err(|e| std::io::Error::other(format!("x509: {e}")))?;
    let not_after = cert.validity().not_after.timestamp();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok((not_after - now) / 86400)
}

fn env_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn env_u16(key: &str, default: u16) -> u16 {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}
