use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::common::admin::AdminAuth;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::featured::service::{FeaturedItem, FeaturedResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/featured", get(pick))
        .route("/admin/featured", get(find_all).post(create))
        .route("/admin/featured/{id}", patch(update).delete(remove))
}

#[derive(Debug, Deserialize)]
struct CreateFeaturedDto {
    #[serde(rename = "type")]
    type_: String,
    #[serde(rename = "scUrn")]
    sc_urn: String,
    #[serde(default)]
    weight: Option<i32>,
    #[serde(default)]
    active: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateFeaturedDto {
    #[serde(default, rename = "type")]
    type_: Option<String>,
    #[serde(default, rename = "scUrn")]
    sc_urn: Option<String>,
    #[serde(default)]
    weight: Option<i32>,
    #[serde(default)]
    active: Option<bool>,
}

async fn pick(
    State(st): State<AppState>,
    ctx: SessionCtx,
) -> AppResult<Json<Option<FeaturedResult>>> {
    Ok(Json(
        st.featured
            .pick(&ctx.session_id.to_string(), &ctx.sc_user_id)
            .await?,
    ))
}

async fn find_all(_: AdminAuth, State(st): State<AppState>) -> AppResult<Json<Vec<FeaturedItem>>> {
    Ok(Json(st.featured.find_all().await?))
}

async fn create(
    _: AdminAuth,
    State(st): State<AppState>,
    Json(dto): Json<CreateFeaturedDto>,
) -> AppResult<Json<FeaturedItem>> {
    Ok(Json(
        st.featured
            .create(&dto.type_, &dto.sc_urn, dto.weight, dto.active)
            .await?,
    ))
}

async fn update(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(dto): Json<UpdateFeaturedDto>,
) -> AppResult<Json<FeaturedItem>> {
    Ok(Json(
        st.featured
            .update(
                &id,
                dto.type_.as_deref(),
                dto.sc_urn.as_deref(),
                dto.weight,
                dto.active,
            )
            .await?,
    ))
}

async fn remove(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    st.featured.remove(&id).await?;
    Ok((StatusCode::OK, Json(json!({ "success": true }))))
}
