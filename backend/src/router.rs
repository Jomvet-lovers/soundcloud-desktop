use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{HeaderName, Method, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::Response;
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::modules;
use crate::state::AppState;

pub fn build(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_methods([
            Method::GET,
            Method::HEAD,
            Method::PUT,
            Method::PATCH,
            Method::POST,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any)
        .expose_headers([HeaderName::from_static("x-session-id")])
        .allow_credentials(false)
        .max_age(Duration::from_secs(3600));

    let http_layer = from_fn_with_state(state.clone(), track_http);

    Router::new()
        .merge(modules::health::router())
        .merge(modules::admin::router())
        .merge(modules::auth::router())
        .merge(modules::me::router())
        .merge(modules::tracks::router())
        .merge(modules::playlists::router())
        .merge(modules::users::router())
        .merge(modules::resolve::router())
        .merge(modules::history::router())
        .merge(modules::events::router())
        .merge(modules::oauth_apps::router())
        .merge(modules::subscriptions::router())
        .merge(modules::auras::router())
        .merge(modules::likes::router())
        .merge(modules::dislikes::router())
        .merge(modules::featured::router())
        .merge(modules::lyrics::router())
        .merge(modules::collab::router())
        .merge(modules::indexing::router())
        .merge(modules::recommendations::router())
        .merge(modules::enrich::router())
        .merge(modules::artists::router())
        .merge(modules::albums::router())
        .merge(modules::discover::router())
        .merge(modules::search::router())
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(60),
        ))
        .layer(TraceLayer::new_for_http())
        .layer(http_layer)
        .layer(cors)
}

/// Records per-route request count + latency into AppState.http_metrics, powering
/// the admin Observability "HTTP RPS / latency" panel.
async fn track_http(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_owned();
    let path = crate::common::http_metrics::normalize_path(req.uri().path());
    let key = format!("{method} {path}");
    let start = std::time::Instant::now();
    let resp = next.run(req).await;
    state
        .http_metrics
        .record(&key, start.elapsed().as_millis() as u64, resp.status().as_u16());
    resp
}
