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
    let counts = sqlx::query_file!("queries/admin/sync_queue/stats_counts.sql")
        .fetch_one(&state.pg)
        .await?;
    let rows = sqlx::query_file!("queries/admin/sync_queue/stats_by_action.sql")
        .fetch_all(&state.pg)
        .await?;
    let by_action = rows
        .into_iter()
        .map(|r| ActionCount {
            action_type: r.action_type,
            count: r.count,
        })
        .collect();
    let recent_errors: Vec<String> =
        sqlx::query_file_scalar!("queries/admin/sync_queue/recent_errors.sql")
            .fetch_all(&state.pg)
            .await?;

    Ok(Json(SyncQueueStats {
        pending: counts.pending,
        failed: counts.failed,
        dead: counts.dead,
        oldest_pending_at: counts.oldest_pending_at,
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
    let res = sqlx::query_file!("queries/admin/sync_queue/flush.sql")
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
    let res = sqlx::query_file!("queries/admin/sync_queue/purge.sql", q.min_retries)
        .execute(&state.pg)
        .await?;
    Ok(Json(PurgeResponse {
        purged: res.rows_affected(),
        min_retries: q.min_retries,
    }))
}
