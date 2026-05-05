use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/events", post(record))
}

#[derive(Debug, Deserialize)]
struct RecordEventDto {
    #[serde(rename = "scUserId")]
    sc_user_id: String,
    #[serde(rename = "scTrackId")]
    sc_track_id: String,
    #[serde(rename = "eventType")]
    event_type: String,
}

async fn record(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Json(body): Json<RecordEventDto>,
) -> AppResult<Json<Value>> {
    st.events
        .record(&body.sc_user_id, &body.sc_track_id, &body.event_type)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
