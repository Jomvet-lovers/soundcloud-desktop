use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Serialize)]
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

    Ok(Json(AuthOverview {
        total,
        valid,
        expired,
        expiring_1h,
        distinct_users,
        active_24h,
    }))
}

#[derive(Serialize, sqlx::FromRow)]
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

    Ok(Json(rows))
}
