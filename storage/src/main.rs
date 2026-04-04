use std::sync::Arc;

use axum::http::Method;
use axum::routing::{delete, get, post};
use axum::Router;
use tokio::sync::Semaphore;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

mod config;
mod routes;
mod transcode;

use config::Config;

pub struct AppState {
    pub config: Config,
    pub transcode_sem: Semaphore,
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

    // Ensure dirs exist
    tokio::fs::create_dir_all(&config.storage_path)
        .await
        .expect("failed to create storage dir");
    tokio::fs::create_dir_all(&config.tmp_path)
        .await
        .expect("failed to create tmp dir");

    let max_transcodes = config.max_transcodes;
    info!(
        "starting storage service on port {}, max_transcodes={}",
        config.port, max_transcodes
    );

    let state = Arc::new(AppState {
        config: config.clone(),
        transcode_sem: Semaphore::new(max_transcodes),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(routes::health::health))
        .route("/upload", post(routes::upload::upload))
        .route("/files/{filename}", delete(routes::files::delete))
        // Serve files: /hq/xxx.ogg, /sq/xxx.ogg
        .route("/{*path}", get(routes::files::serve))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port))
        .await
        .expect("failed to bind");

    info!("listening on 0.0.0.0:{}", config.port);
    axum::serve(listener, app).await.expect("server error");
}
