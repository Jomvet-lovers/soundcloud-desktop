use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::history::service::{HistoryPage, RecordHistoryDto};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/history", get(find_all).post(record).delete(clear))
}

#[derive(Debug, Clone, Deserialize)]
struct PageQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    offset: Option<String>,
}

async fn record(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Json(body): Json<RecordHistoryDto>,
) -> AppResult<StatusCode> {
    st.history.record(&ctx.sc_user_id, &body).await?;
    Ok(StatusCode::OK)
}

async fn find_all(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PageQuery>,
) -> AppResult<Json<HistoryPage>> {
    let limit = q
        .limit
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50)
        .min(200);
    let offset = q
        .offset
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    Ok(Json(
        st.history.find_all(&ctx.sc_user_id, limit, offset).await?,
    ))
}

async fn clear(State(st): State<AppState>, ctx: SessionCtx) -> AppResult<StatusCode> {
    st.history.clear(&ctx.sc_user_id).await?;
    Ok(StatusCode::OK)
}
