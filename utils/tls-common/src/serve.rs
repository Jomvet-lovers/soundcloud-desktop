use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use rustls::ServerConfig;
use rustls_acme::caches::DirCache;
use rustls_acme::AcmeConfig;
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

use crate::acceptor::{ConnectInfoAcceptor, TrustedProxies};
use crate::config::TlsConfig;
use crate::redirect::redirect_router;
use crate::shutdown::shutdown_signal;

pub async fn serve(cfg: TlsConfig, app: Router) {
    let _ = rustls::crypto::ring::default_provider().install_default();

    if let Err(e) = tokio::fs::create_dir_all(&cfg.cache_dir).await {
        warn!("failed to create ACME cache dir {:?}: {}", cfg.cache_dir, e);
    }

    let mut state = AcmeConfig::new(cfg.domains.clone())
        .contact_push(format!("mailto:{}", cfg.email))
        .cache(DirCache::new(cfg.cache_dir.clone()))
        .directory_lets_encrypt(!cfg.staging)
        .state();

    // ALPN h2/http1.1 — без этого rustls-acme отдаёт пустой ALPN, и tonic
    // (и любой h2-only клиент) валится "HTTP/2 was not negotiated".
    let mut rustls_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(state.resolver());
    rustls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let trusted = TrustedProxies {
        cidrs: Arc::new(cfg.proxy_trusted_cidrs.clone()),
        resolved: Arc::new(std::sync::RwLock::new(Vec::new())),
    };
    if !cfg.proxy_trusted_hosts.is_empty() {
        let hosts = cfg.proxy_trusted_hosts.clone();
        let resolved = trusted.resolved.clone();
        tokio::spawn(async move {
            loop {
                let mut ips = Vec::new();
                for h in &hosts {
                    if let Ok(addrs) = tokio::net::lookup_host(format!("{h}:0")).await {
                        ips.extend(addrs.map(|sa| sa.ip()));
                    }
                }
                if let Ok(mut w) = resolved.write() {
                    *w = ips;
                }
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
    }
    let acceptor = ConnectInfoAcceptor {
        inner: state.axum_acceptor(Arc::new(rustls_config)),
        proxy_protocol: cfg.proxy_protocol,
        proxy_trusted: trusted,
    };

    tokio::spawn(async move {
        while let Some(res) = state.next().await {
            match res {
                Ok(ok) => info!("acme event: {:?}", ok),
                Err(err) => error!("acme error: {:?}", err),
            }
        }
    });

    let https_addr = SocketAddr::from(([0, 0, 0, 0], cfg.https_port));
    let http_addr = SocketAddr::from(([0, 0, 0, 0], cfg.http_port));

    info!(
        "TLS: {} domain(s), https=:{} http=:{} redirect={} staging={} proxy_protocol={}",
        cfg.domains.len(),
        cfg.https_port,
        cfg.http_port,
        cfg.http_redirect,
        cfg.staging,
        cfg.proxy_protocol,
    );

    let http_app = if cfg.http_redirect {
        redirect_router(cfg.https_port)
    } else {
        app.clone()
    };

    let http_handle = axum_server::Handle::new();
    let https_handle = axum_server::Handle::new();

    let shutdown_handles = (http_handle.clone(), https_handle.clone());
    tokio::spawn(async move {
        shutdown_signal().await;
        // 3s grace для in-flight, потом drop.
        let grace = Duration::from_secs(3);
        shutdown_handles.0.graceful_shutdown(Some(grace));
        shutdown_handles.1.graceful_shutdown(Some(grace));
    });

    let http_port = cfg.http_port;
    let http_task = tokio::spawn(async move {
        if let Err(e) = axum_server::bind(http_addr)
            .handle(http_handle)
            .serve(http_app.into_make_service())
            .await
        {
            error!("HTTP :{} server error: {}", http_port, e);
        }
    });

    let https_port = cfg.https_port;
    let https_task = tokio::spawn(async move {
        if let Err(e) = axum_server::bind(https_addr)
            .handle(https_handle)
            .acceptor(acceptor)
            .serve(app.into_make_service())
            .await
        {
            error!("HTTPS :{} server error: {}", https_port, e);
        }
    });

    let _ = tokio::join!(http_task, https_task);
}
