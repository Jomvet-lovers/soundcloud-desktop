use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::dislikes::service::{DislikesPage, StatusResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/dislikes/{sc_track_id}", post(add).delete(remove))
        .route("/dislikes/status/{sc_track_id}", get(status))
        .route("/dislikes/ids", get(ids))
        .route("/dislikes", get(list))
}

#[derive(Debug, Clone, Deserialize)]
struct PageQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
}

async fn add(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(sc_track_id): Path<String>,
    body: Option<Json<Value>>,
) -> AppResult<(StatusCode, Json<StatusResult>)> {
    let track_data = body.map(|Json(v)| v);
    let result = st
        .dislikes
        .add(&ctx.sc_user_id, &sc_track_id, track_data.as_ref())
        .await?;
    Ok((StatusCode::OK, Json(result)))
}

async fn remove(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(sc_track_id): Path<String>,
) -> AppResult<Json<StatusResult>> {
    Ok(Json(st.dislikes.remove(&ctx.sc_user_id, &sc_track_id).await?))
}

async fn status(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(sc_track_id): Path<String>,
) -> AppResult<Json<Value>> {
    let disliked = st.dislikes.is_disliked(&ctx.sc_user_id, &sc_track_id).await?;
    Ok(Json(json!({ "disliked": disliked })))
}

async fn ids(State(st): State<AppState>, ctx: SessionCtx) -> AppResult<Json<Value>> {
    let ids = st.dislikes.list_ids_by_user_id(&ctx.sc_user_id, 1000).await?;
    Ok(Json(json!({ "ids": ids })))
}

async fn list(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PageQuery>,
) -> AppResult<Json<DislikesPage>> {
    let limit = q
        .limit
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50)
        .min(200);
    Ok(Json(
        st.dislikes
            .find_all(&ctx.sc_user_id, limit, q.cursor.as_deref())
            .await?,
    ))
}
