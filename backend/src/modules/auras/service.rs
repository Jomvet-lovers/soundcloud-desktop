use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::error::{AppError, AppResult};
use crate::modules::subscriptions::SubscriptionsService;

const ALLOWED_IDS: &[&str] = &[
    "aurora", "magma", "cyber", "void", "sunset", "forest", "ocean", "custom",
];

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Aura {
    pub aura_id: String,
    pub custom_hex: Option<String>,
}

pub struct AurasService {
    pg: PgPool,
    subscriptions: Arc<SubscriptionsService>,
}

impl AurasService {
    pub fn new(pg: PgPool, subscriptions: Arc<SubscriptionsService>) -> Arc<Self> {
        Arc::new(Self { pg, subscriptions })
    }

    pub async fn get(&self, user_urn: &str) -> AppResult<Option<Aura>> {
        let row: Option<Aura> =
            sqlx::query_as("SELECT aura_id, custom_hex FROM user_auras WHERE user_urn = $1")
                .bind(user_urn)
                .fetch_optional(&self.pg)
                .await?;
        Ok(row)
    }

    pub async fn upsert(
        &self,
        user_urn: &str,
        aura_id: &str,
        custom_hex: Option<&str>,
    ) -> AppResult<Aura> {
        if !ALLOWED_IDS.contains(&aura_id) {
            return Err(AppError::bad_request("Unknown aura id"));
        }
        if aura_id == "custom" {
            let hex = custom_hex.ok_or_else(|| AppError::bad_request("Missing custom_hex"))?;
            if !is_valid_hex(hex) {
                return Err(AppError::bad_request("Invalid custom_hex"));
            }
        }
        if !self.subscriptions.is_premium(user_urn).await? {
            return Err(AppError::bad_request("Star subscription required"));
        }
        let stored_hex = if aura_id == "custom" { custom_hex } else { None };
        sqlx::query(
            "INSERT INTO user_auras (user_urn, aura_id, custom_hex, updated_at) \
             VALUES ($1, $2, $3, NOW()) \
             ON CONFLICT (user_urn) DO UPDATE \
             SET aura_id = EXCLUDED.aura_id, custom_hex = EXCLUDED.custom_hex, updated_at = NOW()",
        )
        .bind(user_urn)
        .bind(aura_id)
        .bind(stored_hex)
        .execute(&self.pg)
        .await?;
        Ok(Aura {
            aura_id: aura_id.to_string(),
            custom_hex: stored_hex.map(|s| s.to_string()),
        })
    }
}

fn is_valid_hex(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}
