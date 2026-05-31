use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/collab/status", get(status))
        .route("/admin/collab/train", post(train))
}

#[derive(Debug, Deserialize, Default)]
struct TrainBody {
    #[serde(default)]
    dim: Option<u32>,
    #[serde(default, rename = "minCount")]
    min_count: Option<u32>,
}

async fn status(_: AdminAuth, State(st): State<AppState>) -> AppResult<Json<Value>> {
    let dim = st.collab_vector.get_collab_dim().await;
    Ok(Json(json!({
        "collection_exists": dim.is_some(),
        "dim": dim,
    })))
}

async fn train(
    _: AdminAuth,
    State(st): State<AppState>,
    body: Option<Json<TrainBody>>,
) -> AppResult<Json<Value>> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let result = st
        .collab_trainer
        .train_now(body.dim, body.min_count)
        .await?;
    Ok(Json(json!({
        "enqueued": result.enqueued,
        "sessions": result.sessions,
        "reason": result.reason,
    })))
}
