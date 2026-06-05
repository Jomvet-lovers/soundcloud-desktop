use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::cache::cache_service::CacheScope;
use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::state::AppState;

const TTL_SEC: u64 = 30;
const OVERVIEW_KEY: &str = "admin:auth:overview:v1";
const OAUTH_HEALTH_KEY: &str = "admin:oauth:health:v1";

#[derive(Serialize, Deserialize)]
pub struct AuthOverview {
    pub total: i64,
    pub valid: i64,
    pub expired: i64,
    pub expiring_1h: i64,
    pub distinct_users: i64,
    pub active_24h: i64,
}

/// GET /admin/auth/overview — session-token health derived from the `sessions`
/// table. `expires_at` is stored as naive UTC, so it is compared against
/// `now() at time zone 'utc'`.
#[tracing::instrument(skip_all)]
pub async fn overview(_: AdminAuth, State(state): State<AppState>) -> AppResult<Json<AuthOverview>> {
    if let Ok(Some(raw)) = state.cache.get_raw(OVERVIEW_KEY).await {
        if let Ok(cached) = serde_json::from_str::<AuthOverview>(&raw) {
            return Ok(Json(cached));
        }
    }

    let (total, valid, expired, expiring_1h, distinct_users, active_24h): (i64, i64, i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT \
               COUNT(*)::int8, \
               COUNT(*) FILTER (WHERE expires_at > (now() at time zone 'utc'))::int8, \
               COUNT(*) FILTER (WHERE expires_at <= (now() at time zone 'utc'))::int8, \
               COUNT(*) FILTER (WHERE expires_at > (now() at time zone 'utc') \
                                  AND expires_at <= (now() at time zone 'utc') + interval '1 hour')::int8, \
               COUNT(DISTINCT soundcloud_user_id)::int8, \
               COUNT(*) FILTER (WHERE updated_at > (now() at time zone 'utc') - interval '24 hours')::int8 \
             FROM sessions",
        )
            .fetch_one(&state.pg)
            .await?;

    let resp = AuthOverview {
        total,
        valid,
        expired,
        expiring_1h,
        distinct_users,
        active_24h,
    };
    if let Ok(payload) = serde_json::to_string(&resp) {
        let _ = state
            .cache
            .set_raw(OVERVIEW_KEY, &payload, TTL_SEC, None, CacheScope::Shared, None)
            .await;
    }
    Ok(Json(resp))
}

#[derive(Serialize, Deserialize, sqlx::FromRow)]
pub struct OAuthAppHealth {
    pub id: uuid::Uuid,
    pub name: String,
    pub client_id: String,
    pub active: bool,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sessions_total: i64,
    pub sessions_active: i64,
    pub sessions_expired: i64,
}

/// GET /admin/oauth-apps/health — per-app session breakdown (sessions reference
/// the app via `sessions.oauth_app_id`, a text mirror of `oauth_apps.id`).
#[tracing::instrument(skip_all)]
pub async fn oauth_health(_: AdminAuth, State(state): State<AppState>) -> AppResult<Json<Vec<OAuthAppHealth>>> {
    if let Ok(Some(raw)) = state.cache.get_raw(OAUTH_HEALTH_KEY).await {
        if let Ok(cached) = serde_json::from_str::<Vec<OAuthAppHealth>>(&raw) {
            return Ok(Json(cached));
        }
    }

    let rows = sqlx::query_as::<_, OAuthAppHealth>(
        "SELECT a.id, a.name, a.client_id, a.active, a.last_used_at, \
                COUNT(s.id)::int8 AS sessions_total, \
                COUNT(s.id) FILTER (WHERE s.expires_at > (now() at time zone 'utc'))::int8 AS sessions_active, \
                COUNT(s.id) FILTER (WHERE s.expires_at <= (now() at time zone 'utc'))::int8 AS sessions_expired \
         FROM oauth_apps a \
         LEFT JOIN sessions s ON s.oauth_app_id = a.id::text \
         GROUP BY a.id, a.name, a.client_id, a.active, a.last_used_at \
         ORDER BY sessions_total DESC, a.name ASC",
    )
        .fetch_all(&state.pg)
        .await?;

    if let Ok(payload) = serde_json::to_string(&rows) {
        let _ = state
            .cache
            .set_raw(OAUTH_HEALTH_KEY, &payload, TTL_SEC, None, CacheScope::Shared, None)
            .await;
    }
    Ok(Json(rows))
}
