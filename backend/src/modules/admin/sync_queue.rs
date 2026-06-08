use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::common::admin::AdminAuth;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Serialize)]
pub struct ActionCount {
    pub action_type: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct SyncQueueStats {
    pub pending: i64,
    pub failed: i64,
    pub dead: i64,
    pub oldest_pending_at: Option<chrono::DateTime<chrono::Utc>>,
    pub by_action: Vec<ActionCount>,
    pub recent_errors: Vec<String>,
}

/// GET /admin/sync-queue — outbox health. A live row is pending work (removed on
/// success); after MAX_RETRIES it is parked (`dead=true`) rather than dropped, so
/// user intent is never lost. `failed` counts errored-or-dead rows; `dead` counts
/// parked ones; `recent_errors` samples `last_error` so a stuck queue is diagnosable.
#[tracing::instrument(skip_all)]
pub async fn get_stats(
    _: AdminAuth,
    State(state): State<AppState>,
) -> AppResult<Json<SyncQueueStats>> {
    let (pending, failed, dead, oldest_pending_at): (
        i64,
        i64,
        i64,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE dead = false)::int8, \
                COUNT(*) FILTER (WHERE retry_count > 0 OR dead = true)::int8, \
                COUNT(*) FILTER (WHERE dead = true)::int8, \
                MIN(created_at) FILTER (WHERE dead = false) \
         FROM sync_queue",
    )
        .fetch_one(&state.pg)
        .await?;
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT action_type, COUNT(*)::int8 FROM sync_queue GROUP BY action_type ORDER BY COUNT(*) DESC",
    )
        .fetch_all(&state.pg)
        .await?;
    let by_action = rows
        .into_iter()
        .map(|(action_type, count)| ActionCount { action_type, count })
        .collect();
    let recent_errors: Vec<String> = sqlx::query_scalar(
        "SELECT last_error FROM sync_queue WHERE last_error IS NOT NULL \
         ORDER BY next_run_at DESC LIMIT 10",
    )
        .fetch_all(&state.pg)
        .await?;

    Ok(Json(SyncQueueStats {
        pending,
        failed,
        dead,
        oldest_pending_at,
        by_action,
        recent_errors,
    }))
}

#[derive(Serialize)]
pub struct FlushResponse {
    pub flushed: u64,
}

/// POST /admin/sync-queue/flush — make idle/backoff rows eligible now so the
/// worker tick picks them up immediately. Skips rows whose lease is still live
/// (mirrors the worker's `LOCK_TIMEOUT`); clearing those would let an in-flight,
/// non-idempotent SC call (comment/playlist_create) be re-dispatched and duplicated.
#[tracing::instrument(skip_all)]
pub async fn flush(_: AdminAuth, State(state): State<AppState>) -> AppResult<Json<FlushResponse>> {
    let res = sqlx::query(
        "UPDATE sync_queue SET next_run_at = now(), locked_at = NULL \
         WHERE locked_at IS NULL OR locked_at < now() - interval '5 minutes'",
    )
        .execute(&state.pg)
        .await?;
    Ok(Json(FlushResponse {
        flushed: res.rows_affected(),
    }))
}

#[derive(Deserialize)]
pub struct PurgeQuery {
    #[serde(default = "default_min_retries")]
    pub min_retries: i32,
}

fn default_min_retries() -> i32 {
    // A row hitting MAX_RETRIES is parked (dead=true, retry_count=MAX_RETRIES),
    // not deleted — so min_retries <= MAX_RETRIES catches both about-to-die and
    // parked rows. Default trims everything from the last retry onward.
    crate::modules::sync_queue::service::MAX_RETRIES - 1
}

#[derive(Serialize)]
pub struct PurgeResponse {
    pub purged: u64,
    pub min_retries: i32,
}

/// POST /admin/sync-queue/purge?min_retries=N — drop rows stuck after >= N retries.
#[tracing::instrument(skip_all)]
pub async fn purge(
    _: AdminAuth,
    State(state): State<AppState>,
    Query(q): Query<PurgeQuery>,
) -> AppResult<Json<PurgeResponse>> {
    if q.min_retries < 1 {
        return Err(AppError::bad_request("min_retries must be >= 1"));
    }
    let res = sqlx::query("DELETE FROM sync_queue WHERE retry_count >= $1")
        .bind(q.min_retries)
        .execute(&state.pg)
        .await?;
    Ok(Json(PurgeResponse {
        purged: res.rows_affected(),
        min_retries: q.min_retries,
    }))
}
