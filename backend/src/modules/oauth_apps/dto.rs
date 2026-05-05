use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::oauth_apps::model::OAuthApp;

#[derive(Debug, Deserialize)]
pub struct CreateOAuthAppDto {
    pub name: String,
    #[serde(rename = "clientId")]
    pub client_id: String,
    #[serde(rename = "clientSecret")]
    pub client_secret: String,
    #[serde(rename = "redirectUri")]
    pub redirect_uri: String,
    #[serde(default)]
    pub active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateOAuthAppDto {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "clientId")]
    pub client_id: Option<String>,
    #[serde(default, rename = "clientSecret")]
    pub client_secret: Option<String>,
    #[serde(default, rename = "redirectUri")]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct OAuthAppResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "clientId")]
    pub client_id: String,
    #[serde(rename = "redirectUri")]
    pub redirect_uri: String,
    pub active: bool,
    #[serde(rename = "lastUsedAt")]
    pub last_used_at: Option<DateTime<Utc>>,
    #[serde(rename = "createdAt")]
    pub created_at: NaiveDateTime,
}

impl From<OAuthApp> for OAuthAppResponse {
    fn from(app: OAuthApp) -> Self {
        Self {
            id: app.id,
            name: app.name,
            client_id: app.client_id,
            redirect_uri: app.redirect_uri,
            active: app.active,
            last_used_at: app.last_used_at,
            created_at: app.created_at,
        }
    }
}
