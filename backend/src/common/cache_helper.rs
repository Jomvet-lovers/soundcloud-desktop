use axum::http::StatusCode;
use axum::response::Response;
use serde_json::Value;

use crate::cache::cache_service::CacheScope;
use crate::common::response::json_response;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub struct CacheOpts<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub scope: CacheScope,
    pub session_id: Option<&'a str>,
    pub ttl_sec: u64,
    pub cache_key: Option<&'a str>,
}

/// Прозрачный read-through cache над любым endpoint'ом, возвращающим JSON.
/// Промах кеша — выполняем `fetch`, кладём результат в Redis, возвращаем.
/// `cache_key` — опциональный bucket для invalidate_by_cache_keys (например
/// для invalidate'a при мутациях по target).
pub async fn cached_or_fetch<F, Fut>(
    st: &AppState,
    opts: CacheOpts<'_>,
    fetch: F,
) -> AppResult<Response>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Value>>,
{
    let key = st
        .cache
        .build_key(opts.method, opts.url, opts.scope, opts.session_id);
    if let Ok(Some(raw)) = st.cache.get_raw(&key).await {
        return Ok(json_response(StatusCode::OK, raw));
    }
    let v = fetch().await?;
    let payload =
        serde_json::to_string(&v).map_err(|e| AppError::internal(format!("json encode: {e}")))?;
    let _ = st
        .cache
        .set_raw(
            &key,
            &payload,
            opts.ttl_sec,
            opts.cache_key,
            opts.scope,
            opts.session_id,
        )
        .await;
    Ok(json_response(StatusCode::OK, payload))
}
