use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pending-actions", get(list))
        .route("/pending-actions/stats", get(stats))
        .route("/pending-actions/sync", post(sync))
}

async fn list(State(st): State<AppState>, ctx: SessionCtx) -> AppResult<Json<Value>> {
    let rows = st.pending_actions.get_for_session(&ctx.session_id.to_string()).await?;
    Ok(Json(json!(rows)))
}

async fn stats(State(st): State<AppState>, ctx: SessionCtx) -> AppResult<Json<Value>> {
    let s = st.pending_actions.get_stats(&ctx.session_id.to_string()).await?;
    Ok(Json(json!(s)))
}

async fn sync(State(st): State<AppState>, ctx: SessionCtx) -> AppResult<Json<Value>> {
    let s = st.pending_actions.sync_for_session(&ctx.session_id.to_string()).await?;
    Ok(Json(json!(s)))
}
