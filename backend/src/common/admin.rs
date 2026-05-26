use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use subtle::ConstantTimeEq;

use crate::error::AppError;
use crate::state::AppState;

pub struct AdminAuth;

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let expected = &state.config.admin.token;
        if expected.is_empty() {
            return Err(AppError::unauthorized("Invalid admin token"));
        }
        let provided = parts
            .headers
            .get("x-admin-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if provided.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() != 1 {
            return Err(AppError::unauthorized("Invalid admin token"));
        }
        Ok(AdminAuth)
    }
}
