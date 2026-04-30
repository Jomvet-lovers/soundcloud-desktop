//! cert-runner — supervises a child process and keeps a TLS cert fresh.
//!
//! Workflow:
//! 1. read TlsConfig from env (same vars as tls-common)
//! 2. ensure a valid cert is on disk; if missing/expiring, ACME-issue with
//!    fallback: HTTP-01 on :80 → TLS-ALPN-01 on :443. Both listeners stay up
//!    during issue so LE multi-perspective probes can reach either path.
//! 3. spawn the child with `TLS_CERT_FILE` / `TLS_KEY_FILE` env pointing at
//!    the materialized PEMs
//! 4. background-watch expiry; near expiry, gracefully stop the child,
//!    re-issue, restart it
//! 5. forward SIGTERM/SIGINT to the child and propagate its exit code
//!
//! When `TLS_ENABLED` is not set, cert-runner just execs the child and acts
//! as a thin pid-1 (no ACME, no env injection).

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info, warn};

mod issuer;

const RENEW_BEFORE_DAYS: i64 = 3;
const RENEWAL_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub domains: Vec<String>,
    pub email: String,
    pub cache_dir: PathBuf,
    pub staging: bool,
    pub https_port: u16,
    pub http_port: u16,
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
        let http_port = env_u16("TLS_HTTP_PORT", 80);
        Some(Self {
            domains,
            email,
            cache_dir,
            staging,
            https_port,
            http_port,
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
            res = &mut renew_rx => match res {
                Ok(()) => Action::RenewNeeded,
                Err(_) => Action::Signal,
            },
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
                warn!(
                    "cert renewal triggered — stopping child to reclaim :{} / :{}",
                    cfg.https_port, cfg.http_port,
                );
                let _ = child.start_kill();
                let _ = child.wait().await;
                // loop → ensure_cert (renew) → respawn child
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
            Err(e) => warn!("[watcher] cert read failed: {e}"),
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
    issuer::issue(cfg).await.map_err(std::io::Error::other)?;
    let days = remaining_days(&cfg.cert_path()).await.unwrap_or(0);
    info!("cert ready ({days}d remaining)");
    Ok(())
}

pub async fn remaining_days(cert_path: &Path) -> std::io::Result<i64> {
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
