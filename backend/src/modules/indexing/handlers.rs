use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::indexing::service::IndexingStats;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/indexing/stats", get(get_stats))
}

async fn get_stats(
    State(st): State<AppState>,
    _ctx: SessionCtx,
) -> AppResult<Json<IndexingStats>> {
    Ok(Json(st.indexing.get_stats().await?))
}
