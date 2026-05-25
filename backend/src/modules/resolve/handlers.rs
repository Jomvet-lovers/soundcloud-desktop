use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::Value;

use crate::cache::cache_service::CacheScope;
use crate::common::response::json_response;
use crate::common::session::OptionalSession;
use crate::error::{AppError, AppResult};
use crate::modules::auth::TokenKind;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/resolve", get(resolve))
}

#[derive(Debug, Clone, Deserialize)]
struct ResolveQuery {
    url: String,
}

async fn resolve(
    State(st): State<AppState>,
    OptionalSession(session): OptionalSession,
    Query(q): Query<ResolveQuery>,
) -> AppResult<Response> {
    let cache_url = format!("/resolve?url={}", q.url);
    let key = st
        .cache
        .build_key("GET", &cache_url, CacheScope::Shared, None);
    if let Ok(Some(raw)) = st.cache.get_raw(&key).await {
        return Ok(json_response(StatusCode::OK, raw));
    }

    let kind = match session.as_ref() {
        Some(s) => TokenKind::UserFirst(s.session_id),
        None => TokenKind::PublicPool,
    };
    let v: Value = st.resolve.resolve(kind, &q.url).await?;
    let payload =
        serde_json::to_string(&v).map_err(|e| AppError::internal(format!("json encode: {e}")))?;
    let _ = st
        .cache
        .set_raw(&key, &payload, 86400, None, CacheScope::Shared, None)
        .await;
    Ok(json_response(StatusCode::OK, payload))
}
