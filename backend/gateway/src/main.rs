mod acme;
mod config;
mod health;
mod lb;
mod proxy;
mod status;
mod supervisor;
mod tls;

use std::env;
use std::process::ExitCode;
use std::time::Instant;

use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info};

use crate::config::Config;
use crate::lb::BackendPool;
use crate::tls::TlsState;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gateway=info,info".parse().unwrap()),
        )
        .init();

    let argv: Vec<_> = env::args_os().skip(1).collect();
    if argv.is_empty() {
        eprintln!("usage: gateway <backend-cmd> [args...]");
        return ExitCode::from(2);
    }

    let cfg = match Config::from_env(argv) {
        Ok(c) => c,
        Err(e) => {
            error!("config: {e}");
            return ExitCode::from(2);
        }
    };
    info!(
        "starting gateway: {} backend(s), tls={}, http=:{}, https=:{}",
        cfg.backend_count, cfg.tls.enabled, cfg.http_port, cfg.https_port,
    );

    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    match run(cfg).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("fatal: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cfg: Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let start = Instant::now();
    let pool = BackendPool::new(cfg.backend_count, &cfg.socket_dir);

    let tls_state = if cfg.tls.enabled {
        acme::ensure_initial_cert(&cfg).await?;
        Some(TlsState::from_disk(&cfg.cert_path(), &cfg.key_path()).await?)
    } else {
        None
    };

    let supervisor = supervisor::start(cfg.clone(), pool.clone()).await?;
    let _health = health::spawn(cfg.clone(), pool.clone());

    let _http = proxy::serve_http(cfg.clone(), pool.clone(), tls_state.clone(), start).await?;
    let _https = match &tls_state {
        Some(ts) => Some(proxy::serve_https(cfg.clone(), pool.clone(), ts.clone(), start).await?),
        None => None,
    };

    let _renew = tls_state.map(|ts| acme::spawn_renew_loop(cfg.clone(), ts));

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM received"),
        _ = sigint.recv() => info!("SIGINT received"),
    }
    info!("graceful shutdown");
    supervisor.shutdown().await;
    Ok(())
}
