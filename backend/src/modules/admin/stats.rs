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

    let (a24, a7, a30, total): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
             COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '24 hours')::int8, \
             COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '7 days')::int8, \
             COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '30 days')::int8, \
             COUNT(*)::int8 \
         FROM sessions",
    )
        .fetch_one(&state.pg)
        .await?;

    let resp = StatsResponse {
        active_24h: a24,
        active_7d: a7,
        active_30d: a30,
        total_sessions: total,
    };

    if let Ok(payload) = serde_json::to_string(&resp) {
        let _ = state
            .cache
            .set_raw(CACHE_KEY, &payload, TTL_SEC, None, CacheScope::Shared, None)
            .await;
    }

    Ok(Json(resp))
}
