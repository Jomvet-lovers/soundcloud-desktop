pub mod stats;
pub mod sync_queue;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/stats", get(stats::get_stats))
        .route("/admin/sync-queue", get(sync_queue::get_stats))
        .route("/admin/sync-queue/flush", post(sync_queue::flush))
        .route("/admin/sync-queue/purge", post(sync_queue::purge))
}
