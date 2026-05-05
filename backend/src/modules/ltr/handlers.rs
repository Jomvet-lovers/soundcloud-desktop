use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/ltr/train", post(train))
}

async fn train(State(st): State<AppState>) -> AppResult<Json<Value>> {
    let r = st.ltr_trainer.train_now().await?;
    Ok(Json(json!({
        "enqueued": r.enqueued,
        "examples": r.examples,
        "reason": r.reason,
    })))
}
