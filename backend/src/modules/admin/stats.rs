use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Serialize)]
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
    let now = chrono::Utc::now().naive_utc();
    let ago_24h = now - chrono::Duration::hours(24);
    let ago_7d = now - chrono::Duration::days(7);
    let ago_30d = now - chrono::Duration::days(30);

    let (a24, a7, a30, total) = tokio::try_join!(
        count_since(&state, Some(ago_24h)),
        count_since(&state, Some(ago_7d)),
        count_since(&state, Some(ago_30d)),
        count_since(&state, None),
    )?;

    Ok(Json(StatsResponse {
        active_24h: a24,
        active_7d: a7,
        active_30d: a30,
        total_sessions: total,
    }))
}

async fn count_since(
    state: &AppState,
    since: Option<chrono::NaiveDateTime>,
) -> AppResult<i64> {
    let n: i64 = if let Some(ts) = since {
        sqlx::query_scalar("SELECT COUNT(*)::int8 FROM sessions WHERE updated_at > $1")
            .bind(ts)
            .fetch_one(&state.pg)
            .await?
    } else {
        sqlx::query_scalar("SELECT COUNT(*)::int8 FROM sessions")
            .fetch_one(&state.pg)
            .await?
    };
    Ok(n)
}
