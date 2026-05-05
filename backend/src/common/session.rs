use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::common::uuid::parse_uuid;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Clone)]
pub struct SessionCtx {
    pub session_id: Uuid,
    pub access_token: String,
    pub sc_user_id: String,
}

impl FromRequestParts<AppState> for SessionCtx {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let raw = extract_session_id(parts)
            .ok_or_else(|| AppError::unauthorized("Missing or malformed x-session-id header"))?;

        let session_id = parse_uuid(&raw)
            .ok_or_else(|| AppError::unauthorized("Missing or malformed x-session-id header"))?;

        let session = state.auth.get_valid_session(session_id).await?;

        let sc_user_id = session.soundcloud_user_id.ok_or_else(|| {
            AppError::unauthorized("Session missing SoundCloud user info, please re-authenticate")
        })?;

        Ok(SessionCtx {
            session_id,
            access_token: session.access_token,
            sc_user_id,
        })
    }
}

#[derive(Clone, Default)]
pub struct OptionalSession(pub Option<SessionCtx>);

impl FromRequestParts<AppState> for OptionalSession {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(raw) = extract_session_id(parts) else {
            return Ok(OptionalSession(None));
        };
        let Some(session_id) = parse_uuid(&raw) else {
            return Ok(OptionalSession(None));
        };
        let Ok(session) = state.auth.get_valid_session(session_id).await else {
            return Ok(OptionalSession(None));
        };
        let Some(sc_user_id) = session.soundcloud_user_id else {
            return Ok(OptionalSession(None));
        };
        Ok(OptionalSession(Some(SessionCtx {
            session_id,
            access_token: session.access_token,
            sc_user_id,
        })))
    }
}

pub struct RawSessionIdHeader(pub Option<String>);

impl FromRequestParts<AppState> for RawSessionIdHeader {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(RawSessionIdHeader(extract_session_id(parts)))
    }
}

fn extract_session_id(parts: &Parts) -> Option<String> {
    if let Some(v) = parts
        .headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
    {
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    if let Some(q) = parts.uri.query() {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "session_id" && !v.is_empty() {
                    return Some(urlencoding::decode(v).ok()?.into_owned());
                }
            }
        }
    }
    None
}
