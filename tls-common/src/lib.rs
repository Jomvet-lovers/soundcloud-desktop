use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Redirect;
use axum::Router;
use rustls_acme::caches::DirCache;
use rustls_acme::AcmeConfig;
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

/// Future that resolves on SIGINT or SIGTERM. Use with
/// `axum::serve(...).with_graceful_shutdown(shutdown_signal())`.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                warn!("failed to install SIGTERM handler: {}", e);
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("SIGINT received, shutting down"),
        _ = terminate => info!("SIGTERM received, shutting down"),
    }
}

pub struct TlsConfig {
    pub domains: Vec<String>,
    pub email: String,
    pub cache_dir: PathBuf,
    pub staging: bool,
    pub https_port: u16,
    pub http_port: u16,
    pub http_redirect: bool,
}

impl TlsConfig {
    /// Returns Some(cfg) when TLS_ENABLED=true, otherwise None.
    /// Panics on misconfiguration (TLS on but DOMAINS empty) — fail fast at boot.
    pub fn from_env() -> Option<Self> {
        if !env_bool("TLS_ENABLED", false) {
            return None;
        }

        let domains = parse_csv(&std::env::var("DOMAINS").unwrap_or_default());
        if domains.is_empty() {
            panic!("TLS_ENABLED=true but DOMAINS is empty (expected comma-separated domain list)");
        }

        let email = std::env::var("ACME_EMAIL")
            .unwrap_or_else(|_| format!("admin@{}", domains[0]));
        let cache_dir = PathBuf::from(
            std::env::var("ACME_CACHE_DIR").unwrap_or_else(|_| "/var/cache/acme".to_string()),
        );
        let staging = env_bool("ACME_STAGING", false);
        let https_port = env_u16("TLS_HTTPS_PORT", 443);
        let http_port = env_u16("TLS_HTTP_PORT", 80);
        // HTTP->HTTPS 301 by default; disable only for explicit mixed-mode.
        let http_redirect = env_bool("TLS_HTTP_REDIRECT", true);

        Some(Self {
            domains,
            email,
            cache_dir,
            staging,
            https_port,
            http_port,
            http_redirect,
        })
    }
}

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

    let rustls_config = state.default_rustls_config();
    let acceptor = state.axum_acceptor(rustls_config);

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
        "TLS: {} domain(s), https=:{} http=:{} redirect={} staging={}",
        cfg.domains.len(),
        cfg.https_port,
        cfg.http_port,
        cfg.http_redirect,
        cfg.staging
    );

    let https_port = cfg.https_port;
    let http_app: Router = if cfg.http_redirect {
        redirect_router(https_port)
    } else {
        app.clone()
    };

    let http_handle = axum_server::Handle::new();
    let https_handle = axum_server::Handle::new();

    let shutdown_handles = (http_handle.clone(), https_handle.clone());
    tokio::spawn(async move {
        shutdown_signal().await;
        // Give in-flight requests up to 3s, then drop.
        let grace = std::time::Duration::from_secs(3);
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

fn redirect_router(https_port: u16) -> Router {
    use axum::handler::HandlerWithoutStateExt;

    let redirect = move |headers: HeaderMap, uri: Uri| async move {
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        if host.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let host_no_port = host.split(':').next().unwrap_or(&host).to_string();
        let authority = if https_port == 443 {
            host_no_port
        } else {
            format!("{host_no_port}:{https_port}")
        };
        let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        match Uri::builder()
            .scheme("https")
            .authority(authority)
            .path_and_query(pq)
            .build()
        {
            Ok(u) => Ok(Redirect::permanent(&u.to_string())),
            Err(_) => Err(StatusCode::BAD_REQUEST),
        }
    };
    Router::new().fallback_service(redirect.into_service())
}

pub fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_csv(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
