pub mod stats;

use axum::routing::get;
use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/stats", get(stats::get_stats))
}
