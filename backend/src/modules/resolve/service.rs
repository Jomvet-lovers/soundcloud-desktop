use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use tracing::debug;

use crate::error::{AppError, AppResult};
use crate::sc::ScClient;

const MAX_RANDOM_TOKEN_ATTEMPTS: i64 = 10;

pub struct ResolveService {
    sc: ScClient,
    pg: PgPool,
}

impl ResolveService {
    pub fn new(sc: ScClient, pg: PgPool) -> Arc<Self> {
        Arc::new(Self { sc, pg })
    }

    pub async fn resolve(&self, token: &str, url: &str) -> AppResult<Value> {
        self.sc
            .api_get_value("/resolve", token, Some(&[("url".into(), url.to_string())]))
            .await
    }

    pub async fn resolve_with_random_token(&self, url: &str) -> AppResult<Value> {
        let now = Utc::now().naive_utc();
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT access_token FROM sessions \
             WHERE access_token <> '' AND expires_at > $1 \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(now)
        .bind(MAX_RANDOM_TOKEN_ATTEMPTS)
        .fetch_all(&self.pg)
        .await?;
        for (token,) in rows {
            match self.resolve(&token, url).await {
                Ok(v) => return Ok(v),
                Err(_) => debug!("Token failed for resolve, trying next..."),
            }
        }
        Err(AppError::internal("No valid session token available for resolve"))
    }
}
