use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::cache::cache_service::CacheScope;
use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::state::AppState;

const CACHE_KEY: &str = "admin:stats:sessions:v1";
const TTL_SEC: u64 = 30;

#[derive(Serialize, Deserialize)]
pub struct StatsResponse {
    pub active_24h: i64,
    pub active_7d: i64,
    pub active_30d: i64,
    pub total_sessions: i64,
}

#[tracing::instrument(skip_all)]
pub async fn get_stats(
    _: AdminAuth,
    State(state): State<AppState>,
) -> AppResult<Json<StatsResponse>> {
    if let Ok(Some(raw)) = state.cache.get_raw(CACHE_KEY).await {
        if let Ok(cached) = serde_json::from_str::<StatsResponse>(&raw) {
            return Ok(Json(cached));
        }
    }

    let row = sqlx::query_file!("queries/admin/stats/sessions.sql")
        .fetch_one(&state.pg)
        .await?;

    let resp = StatsResponse {
        active_24h: row.active_24h,
        active_7d: row.active_7d,
        active_30d: row.active_30d,
        total_sessions: row.total,
    };

    if let Ok(payload) = serde_json::to_string(&resp) {
        let _ = state
            .cache
            .set_raw(CACHE_KEY, &payload, TTL_SEC, None, CacheScope::Shared, None)
            .await;
    }

    Ok(Json(resp))
}
