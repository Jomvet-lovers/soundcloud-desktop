use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use crate::cache::cache_service::CacheScope;
use crate::common::response::json_response;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Прозрачный read-through cache над любым endpoint'ом, возвращающим JSON.
/// Промах кеша — выполняем `fetch`, кладём результат в Redis, возвращаем.
/// `cache_key` — опциональный bucket для invalidate_by_cache_keys (например
/// для invalidate'a при мутациях по target).
pub async fn cached_or_fetch<F, Fut>(
    st: &AppState,
    method: &str,
    url: &str,
    scope: CacheScope,
    session_id: Option<&str>,
    ttl_sec: u64,
    cache_key: Option<&str>,
    fetch: F,
) -> AppResult<Response>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Value>>,
{
    let key = st.cache.build_key(method, url, scope, session_id);
    if let Ok(Some(raw)) = st.cache.get_raw(&key).await {
        return Ok(json_response(StatusCode::OK, raw));
    }
    let v = fetch().await?;
    let payload =
        serde_json::to_string(&v).map_err(|e| AppError::internal(format!("json encode: {e}")))?;
    let _ = st
        .cache
        .set_raw(&key, &payload, ttl_sec, cache_key, scope, session_id)
        .await;
    Ok(json_response(StatusCode::OK, payload))
}
