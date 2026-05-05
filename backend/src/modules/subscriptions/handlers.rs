use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::modules::subscriptions::service::Subscription;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/subscriptions", get(list).post(upsert))
        .route("/admin/subscriptions/{user_urn}", axum::routing::delete(remove))
}

#[derive(Debug, Deserialize)]
struct UpsertBody {
    user_urn: String,
    exp_date: i64,
}

async fn list(
    _: AdminAuth,
    State(st): State<AppState>,
) -> AppResult<Json<Vec<Subscription>>> {
    Ok(Json(st.subscriptions.list().await?))
}

async fn upsert(
    _: AdminAuth,
    State(st): State<AppState>,
    Json(body): Json<UpsertBody>,
) -> AppResult<Json<Value>> {
    st.subscriptions.upsert(&body.user_urn, body.exp_date).await?;
    Ok(Json(json!({ "message": "ok" })))
}

async fn remove(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(user_urn): Path<String>,
) -> AppResult<Json<Value>> {
    let deleted = st.subscriptions.remove(&user_urn).await?;
    Ok(Json(json!({ "deleted": deleted })))
}
