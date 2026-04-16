use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use axum::extract::DefaultBodyLimit;
use axum::http::Method;
use axum::routing::{delete, get, post};
use axum::Router;
use tokio::sync::Semaphore;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

mod backend;
mod config;
mod routes;
mod transcode;

use backend::{Backend, LocalBackend, S3Backend};
use config::{BackendKind, Config};

pub struct AppState {
    pub config: Config,
    pub backend: Backend,
    pub transcode_sem: Semaphore,
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

    // tmp dir always needed for transcoding
    tokio::fs::create_dir_all(&config.tmp_path)
        .await
        .expect("failed to create tmp dir");

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

    let max_transcodes = config.max_transcodes;
    info!(
        "starting storage service on port {}, max_transcodes={}, ffmpeg_bin={}, ffprobe_bin={}",
        config.port, max_transcodes, config.ffmpeg_bin, config.ffprobe_bin
    );

    let state = Arc::new(AppState {
        config: config.clone(),
        backend,
        transcode_sem: Semaphore::new(max_transcodes),
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
        // Serve files: /hq/xxx.ogg, /sq/xxx.ogg — GET streams, HEAD only checks.
        .route(
            "/{*path}",
            get(routes::files::serve).head(routes::files::head),
        )
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port))
        .await
        .expect("failed to bind");

    info!("listening on 0.0.0.0:{}", config.port);
    axum::serve(listener, app).await.expect("server error");
}
