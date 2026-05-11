use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::auras::service::Aura;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users/{user_urn}/aura", get(get_aura))
        .route("/me/aura", put(put_aura))
}

#[derive(Debug, Serialize)]
struct AuraResponse {
    aura_id: Option<String>,
    custom_hex: Option<String>,
}

impl From<Option<Aura>> for AuraResponse {
    fn from(value: Option<Aura>) -> Self {
        match value {
            Some(a) => Self {
                aura_id: Some(a.aura_id),
                custom_hex: a.custom_hex,
            },
            None => Self {
                aura_id: None,
                custom_hex: None,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct PutAuraBody {
    aura_id: String,
    #[serde(default)]
    custom_hex: Option<String>,
}

async fn get_aura(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Json<AuraResponse>> {
    let row = st.auras.get(&user_urn).await?;
    if row.is_some() && !st.subscriptions.is_premium(&user_urn).await? {
        return Ok(Json(AuraResponse {
            aura_id: None,
            custom_hex: None,
        }));
    }
    Ok(Json(row.into()))
}

async fn put_aura(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Json(body): Json<PutAuraBody>,
) -> AppResult<Json<Value>> {
    let aura = st
        .auras
        .upsert(&ctx.sc_user_id, &body.aura_id, body.custom_hex.as_deref())
        .await?;
    Ok(Json(serde_json::json!({
        "aura_id": aura.aura_id,
        "custom_hex": aura.custom_hex,
    })))
}
