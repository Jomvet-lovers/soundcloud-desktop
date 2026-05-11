use std::time::Duration;

use axum::http::{HeaderName, Method, StatusCode};
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

    Router::new()
        .merge(modules::health::router())
        .merge(modules::admin::router())
        .merge(modules::auth::router())
        .merge(modules::me::router())
        .merge(modules::tracks::router())
        .merge(modules::playlists::router())
        .merge(modules::users::router())
        .merge(modules::pending_actions::router())
        .merge(modules::reposts::router())
        .merge(modules::resolve::router())
        .merge(modules::history::router())
        .merge(modules::events::router())
        .merge(modules::local_likes::router())
        .merge(modules::oauth_apps::router())
        .merge(modules::subscriptions::router())
        .merge(modules::auras::router())
        .merge(modules::likes::router())
        .merge(modules::dislikes::router())
        .merge(modules::featured::router())
        .merge(modules::lyrics::router())
        .merge(modules::collab::router())
        .merge(modules::ltr::router())
        .merge(modules::indexing::router())
        .merge(modules::recommendations::router())
        .merge(modules::enrich::router())
        .merge(modules::artists::router())
        .merge(modules::albums::router())
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(60),
        ))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
