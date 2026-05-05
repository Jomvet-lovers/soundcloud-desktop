use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::modules::oauth_apps::dto::{
    CreateOAuthAppDto, OAuthAppResponse, UpdateOAuthAppDto,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/oauth-apps", get(find_all).post(create))
        .route("/oauth-apps/{id}", patch(update).delete(remove))
}

async fn find_all(
    _: AdminAuth,
    State(st): State<AppState>,
) -> AppResult<Json<Vec<OAuthAppResponse>>> {
    let apps = st.oauth_apps.find_all().await?;
    Ok(Json(apps.into_iter().map(OAuthAppResponse::from).collect()))
}

async fn create(
    _: AdminAuth,
    State(st): State<AppState>,
    Json(dto): Json<CreateOAuthAppDto>,
) -> AppResult<Json<OAuthAppResponse>> {
    let app = st
        .oauth_apps
        .create(
            &dto.name,
            &dto.client_id,
            &dto.client_secret,
            &dto.redirect_uri,
            dto.active,
        )
        .await?;
    Ok(Json(app.into()))
}

async fn update(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(dto): Json<UpdateOAuthAppDto>,
) -> AppResult<Json<OAuthAppResponse>> {
    let app = st
        .oauth_apps
        .update(
            &id,
            dto.name.as_deref(),
            dto.client_id.as_deref(),
            dto.client_secret.as_deref(),
            dto.redirect_uri.as_deref(),
            dto.active,
        )
        .await?;
    Ok(Json(app.into()))
}

async fn remove(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    st.oauth_apps.remove(&id).await?;
    Ok(Json(json!({ "success": true })))
}
