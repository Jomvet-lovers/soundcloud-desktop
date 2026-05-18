use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Session {
    pub id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: NaiveDateTime,
    pub scope: String,
    pub soundcloud_user_id: Option<String>,
    pub username: Option<String>,
    pub oauth_app_id: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub struct LoginRequest {
    pub id: Uuid,
    pub code_verifier: String,
    pub oauth_app_id: Option<String>,
    pub target_session_id: Option<Uuid>,
    pub status: String,
    pub step: Option<String>,
    pub username: Option<String>,
    pub result_session_id: Option<Uuid>,
    pub error: Option<String>,
    pub retry_count: i32,
    pub redirect_url: Option<String>,
    pub expires_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub struct LinkRequestRow {
    pub id: Uuid,
    pub mode: String,
    pub source_session_id: Option<Uuid>,
    pub target_session_id: Option<Uuid>,
    pub status: String,
    pub error: Option<String>,
    pub expires_at: NaiveDateTime,
}
