use axum::Router;
use axum::extract::{Json, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use sc_core::ScRuntime;
use serde_json::Value;
use tower_http::cors::CorsLayer;

use crate::dispatch;
use crate::events;

#[derive(Clone)]
pub struct AppState {
    pub rt: ScRuntime,
    pub token: String,
}

pub fn router(rt: ScRuntime, token: String) -> Router {
    // loopback-бинд + токен-гейт; CORS открыт, чтобы дев-превью (vite-origin)
    // и app://-шелл ходили без исключений
    Router::new()
        .route("/rpc/{method}", post(rpc))
        .route("/events", get(events::ws))
        .layer(CorsLayer::very_permissive())
        .with_state(AppState { rt, token })
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn rpc(
    State(state): State<AppState>,
    Path(method): Path<String>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Response {
    if bearer(&headers) != Some(state.token.as_str()) {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    let args = body.map(|Json(v)| v).unwrap_or(Value::Null);
    match dispatch::dispatch(&state.rt, &method, args).await {
        Ok(value) => Json(value).into_response(),
        Err(err) => err.into_response(),
    }
}
