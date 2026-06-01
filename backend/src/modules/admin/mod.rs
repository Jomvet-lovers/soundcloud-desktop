pub mod auth_overview;
pub mod stats;
pub mod sync_queue;
pub mod wanted;

use axum::routing::{get, patch, post};
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/stats", get(stats::get_stats))
        .route("/admin/sync-queue", get(sync_queue::get_stats))
        .route("/admin/sync-queue/flush", post(sync_queue::flush))
        .route("/admin/sync-queue/purge", post(sync_queue::purge))
        .route("/admin/auth/overview", get(auth_overview::overview))
        .route("/admin/oauth-apps/health", get(auth_overview::oauth_health))
        .route("/admin/wanted-tracks", get(wanted::list))
        .route("/admin/wanted-tracks/{id}/link", post(wanted::link))
        .route("/admin/wanted-tracks/{id}/status", patch(wanted::set_status))
}
