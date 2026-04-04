use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::AppState;

fn require_admin(headers: &HeaderMap, admin_token: &str) -> Result<(), AppError> {
    if admin_token.is_empty() {
        return Err(AppError::Forbidden);
    }
    let token = headers
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    if token != admin_token {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// GET /admin/subscriptions
pub async fn list_subscriptions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    require_admin(&headers, &state.config.admin_token)?;
    let subs = state.sqlite.list_subscriptions()?;
    Ok(Json(subs))
}

#[derive(Deserialize)]
pub struct UpsertSubscriptionBody {
    pub user_urn: String,
    pub exp_date: i64,
}

/// POST /admin/subscriptions
pub async fn upsert_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UpsertSubscriptionBody>,
) -> Result<impl IntoResponse, AppError> {
    require_admin(&headers, &state.config.admin_token)?;
    state
        .sqlite
        .upsert_subscription(&body.user_urn, body.exp_date)?;
    Ok((
        StatusCode::OK,
        Json(serde_json::json!({"message": "ok"})),
    ))
}

/// DELETE /admin/subscriptions/:user_urn
pub async fn delete_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(user_urn): axum::extract::Path<String>,
) -> Result<impl IntoResponse, AppError> {
    require_admin(&headers, &state.config.admin_token)?;
    let deleted = state.sqlite.delete_subscription(&user_urn)?;
    if deleted {
        Ok(Json(serde_json::json!({"message": "deleted"})))
    } else {
        Err(AppError::NotFound)
    }
}
