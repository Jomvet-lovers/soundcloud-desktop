use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize)]
pub struct LoginResponse {
    pub url: String,
    #[serde(rename = "loginRequestId")]
    pub login_request_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct LoginStatusQuery {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub authenticated: bool,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(rename = "soundcloudUserId", skip_serializing_if = "Option::is_none")]
    pub soundcloud_user_id: Option<String>,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<NaiveDateTime>,
}

#[derive(Serialize)]
pub struct RefreshResponse {
    #[serde(rename = "sessionId")]
    pub session_id: Uuid,
    #[serde(rename = "expiresAt")]
    pub expires_at: NaiveDateTime,
}

#[derive(Serialize)]
pub struct LogoutResponse {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateLinkRequest {
    pub mode: String,
}

#[derive(Serialize)]
pub struct CreateLinkResponse {
    #[serde(rename = "linkRequestId")]
    pub link_request_id: Uuid,
    #[serde(rename = "claimToken")]
    pub claim_token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct ClaimLinkRequest {
    #[serde(rename = "claimToken")]
    pub claim_token: String,
}

#[derive(Serialize)]
pub struct ClaimLinkResponse {
    #[serde(rename = "sessionId")]
    pub session_id: Uuid,
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct LinkStatusQuery {
    pub id: String,
}

#[derive(Serialize)]
pub struct LinkStatusResponse {
    pub status: String,
    pub mode: String,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
