use std::sync::Arc;

use axum::http::Method;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

mod admin;
mod cleanup;
mod config;
mod db;
mod error;
mod stream;

use config::Config;
use db::postgres::PgPool;
use db::sqlite::SqliteDb;
use stream::anon::AnonClient;
use stream::cdn::CdnClient;
use stream::cookies::CookiesClient;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pg: PgPool,
    pub sqlite: Arc<SqliteDb>,
    pub http_client: reqwest::Client,
    pub anon: Arc<AnonClient>,
    pub cookies: Option<Arc<CookiesClient>>,
    pub cdn: Arc<CdnClient>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "streaming=info,tower_http=info".parse().unwrap()),
        )
        .init();

    let config = Config::from_env();

    // PostgreSQL
    let pg = PgPool::connect(&config)
        .await
        .expect("Failed to connect to PostgreSQL");

    // SQLite
    let sqlite = Arc::new(SqliteDb::open(&config.sqlite_path).expect("Failed to open SQLite"));

    // HTTP client
    let http_client = reqwest::Client::builder()
        .tcp_nodelay(true)
        .pool_max_idle_per_host(16)
        .connect_timeout(std::time::Duration::from_millis(3000))
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("Failed to build HTTP client");

    // Anon client (shared client_id cache)
    let anon = Arc::new(AnonClient::new(
        http_client.clone(),
        config.sc_proxy_url.clone(),
    ));

    // Cookies client (optional)
    let cookies = if config.cookies_enabled() {
        Some(Arc::new(CookiesClient::new(
            http_client.clone(),
            config.sc_proxy_url.clone(),
            config.sc_cookies.clone(),
            config.sc_oauth_token.clone().unwrap(),
            AnonClient::new(http_client.clone(), config.sc_proxy_url.clone()),
        )))
    } else {
        info!("Cookie-based streaming disabled (SC_COOKIES not set)");
        None
    };

    // CDN client
    let cdn = Arc::new(CdnClient::new(http_client.clone(), &config, pg.clone()));

    if cdn.enabled() {
        info!("CDN enabled: {}", config.cdn_base_url);
    } else {
        info!("CDN disabled");
    }

    let config = Arc::new(config);

    // Spawn cleanup task
    cleanup::task::spawn_cleanup_task((*config).clone(), pg.clone(), cdn.clone());

    let state = AppState {
        config: config.clone(),
        pg,
        sqlite,
        http_client,
        anon,
        cookies,
        cdn,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any)
        .max_age(std::time::Duration::from_secs(3600));

    let app = Router::new()
        // Stream endpoints
        .route("/stream/{track_urn}", get(stream::handler::stream_normal))
        .route(
            "/stream/{track_urn}/premium",
            get(stream::handler::stream_premium),
        )
        // Admin endpoints
        .route(
            "/admin/subscriptions",
            get(admin::handler::list_subscriptions),
        )
        .route(
            "/admin/subscriptions",
            post(admin::handler::upsert_subscription),
        )
        .route(
            "/admin/subscriptions/{user_urn}",
            delete(admin::handler::delete_subscription),
        )
        // Health
        .route("/health", get(|| async { "ok" }))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    info!("Streaming service starting on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}
