use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct OAuthApp {
    pub id: Uuid,
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub active: bool,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
