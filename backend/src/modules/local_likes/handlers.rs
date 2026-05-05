use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::local_likes::service::FindAllResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/local-likes/{sc_track_id}",
            post(add).delete(remove),
        )
        .route("/local-likes", get(find_all))
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
    Json(track_data): Json<Value>,
) -> AppResult<StatusCode> {
    st.local_likes
        .add(&ctx.sc_user_id, &sc_track_id, &track_data)
        .await?;
    Ok(StatusCode::OK)
}

async fn remove(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(sc_track_id): Path<String>,
) -> AppResult<StatusCode> {
    st.local_likes.remove(&ctx.sc_user_id, &sc_track_id).await?;
    Ok(StatusCode::OK)
}

async fn find_all(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PageQuery>,
) -> AppResult<Json<FindAllResult>> {
    let limit = q
        .limit
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50)
        .min(200);
    Ok(Json(
        st.local_likes
            .find_all(&ctx.sc_user_id, limit, q.cursor.as_deref())
            .await?,
    ))
}
