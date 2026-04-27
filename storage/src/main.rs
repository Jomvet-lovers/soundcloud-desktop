use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use axum::extract::DefaultBodyLimit;
use axum::http::Method;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

mod backend;
mod bus;
mod config;
mod pipeline;
mod routes;
mod transcode;

use backend::{Backend, LocalBackend, S3Backend};
use bus::BusClient;
use config::{BackendKind, Config};
use pipeline::Pipeline;

pub struct AppState {
    pub config: Arc<Config>,
    pub backend: Arc<Backend>,
    pub pipeline: Pipeline,
    /// Live byte counter of files in `{tmp}/source/`. Seeded once at startup;
    /// mutated via fetch_add/sub on the hot path. No stat syscalls per request.
    pub tmp_used_bytes: AtomicU64,
    /// Guards a lazy rescan of `{tmp}/source/` to recover from external cleanup.
    pub tmp_rescan_lock: tokio::sync::Mutex<Instant>,
    file_locks: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
}

impl AppState {
    pub fn file_lock(&self, filename: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.file_locks.lock().unwrap();
        if let Some(lock) = locks.get(filename).and_then(Weak::upgrade) {
            return lock;
        }

        locks.retain(|_, lock| lock.upgrade().is_some());

        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(filename.to_string(), Arc::downgrade(&lock));
        lock
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "storage=info,tower_http=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env();
    transcode::validate_binaries(&config.ffmpeg_bin, &config.ffprobe_bin)
        .await
        .expect("ffmpeg/ffprobe validation failed");

    let source_dir = config.source_path();
    let result_dir = config.result_path();
    tokio::fs::create_dir_all(&source_dir)
        .await
        .expect("failed to create source dir");
    tokio::fs::create_dir_all(&result_dir)
        .await
        .expect("failed to create result dir");

    // Wipe stale tmp from previous crashes — these files are not referenced by anyone.
    purge_dir(&source_dir).await;
    purge_dir(&result_dir).await;

    let tmp_used_initial = if config.tmp_max_bytes.is_some() {
        routes::upload::dir_size_bytes(&source_dir).await.unwrap_or(0)
    } else {
        0
    };
    if let Some(limit) = config.tmp_max_bytes {
        info!(
            "tmp initial usage: {:.2} GiB (limit {:.2} GiB)",
            tmp_used_initial as f64 / (1024.0 * 1024.0 * 1024.0),
            limit as f64 / (1024.0 * 1024.0 * 1024.0),
        );
    }

    let backend = match config.backend {
        BackendKind::Local => {
            let b = LocalBackend::new(&config.storage_path)
                .await
                .expect("failed to init local backend");
            info!("backend=local storage_path={}", config.storage_path);
            Backend::Local(b)
        }
        BackendKind::S3 => {
            let s3_cfg = config.s3.as_ref().expect("S3 config missing");
            let b = S3Backend::new(s3_cfg).await;
            info!(
                "backend=s3 bucket={} endpoint={:?} region={}",
                s3_cfg.bucket, s3_cfg.endpoint, s3_cfg.region
            );
            Backend::S3(b)
        }
    };
    let backend = Arc::new(backend);

    info!(
        "starting storage on :{} max_transcodes={} batch_size={} batch_wait_ms={} upload_concurrency={} upload_retries={}",
        config.port,
        config.max_transcodes,
        config.transcode_batch_size,
        config.transcode_batch_wait_ms,
        config.upload_concurrency,
        config.upload_retries,
    );

    let config = Arc::new(config);
    let bus = BusClient::connect(&config.nats_url).await;
    if bus.enabled() && config.event_base_url.is_empty() {
        warn!("NATS connected but EVENT_BASE_URL is empty — track_uploaded events will be skipped");
    }
    let pipeline = Pipeline::start(config.clone(), backend.clone(), bus);

    let state = Arc::new(AppState {
        config: config.clone(),
        backend,
        pipeline,
        tmp_used_bytes: AtomicU64::new(tmp_used_initial),
        tmp_rescan_lock: tokio::sync::Mutex::new(Instant::now()),
        file_locks: Mutex::new(HashMap::new()),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::HEAD, Method::POST, Method::DELETE])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(routes::health::health))
        .route(
            "/upload",
            post(routes::upload::upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/files/{filename}", delete(routes::files::delete))
        .route("/redirect/{*path}", get(routes::files::redirect))
        .route(
            "/{*path}",
            get(routes::files::serve).head(routes::files::head),
        )
        .layer(cors)
        .with_state(state);

    if let Some(tls_cfg) = tls_common::TlsConfig::from_env() {
        tls_common::serve(tls_cfg, app).await;
    } else {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port))
            .await
            .expect("failed to bind");

        info!("listening on 0.0.0.0:{}", config.port);
        axum::serve(listener, app)
            .with_graceful_shutdown(tls_common::shutdown_signal())
            .await
            .expect("server error");
    }
}

/// Best-effort wipe of all top-level files in a tmp dir (called once at boot
/// — anything left from a previous run is unowned and cannot be resumed).
async fn purge_dir(path: &str) {
    let mut rd = match tokio::fs::read_dir(path).await {
        Ok(rd) => rd,
        Err(_) => return,
    };
    let mut count = 0u64;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if ft.is_file() && tokio::fs::remove_file(entry.path()).await.is_ok() {
            count += 1;
        }
    }
    if count > 0 {
        warn!("[boot] purged {count} stale files from {path}");
    }
}
