use std::sync::Arc;

use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use tracing::debug;

use super::anon::{normalize_v2_to_v1, AnonResolveClient};
use crate::error::{AppError, AppResult};
use crate::sc::ScClient;

const MAX_DIRECT_ATTEMPTS: usize = 5;

pub struct ResolveService {
    sc: ScClient,
    pg: PgPool,
    anon: AnonResolveClient,
}

impl ResolveService {
    pub fn new(sc: ScClient, pg: PgPool) -> Arc<Self> {
        let anon = AnonResolveClient::new(sc.clone());
        Arc::new(Self { sc, pg, anon })
    }

    pub async fn resolve(&self, user_token: Option<&str>, url: &str) -> AppResult<Value> {
        let tokens = self.collect_direct_tokens(user_token).await?;

        let direct_err = match self.try_direct(&tokens, url).await {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };

        match self.anon.resolve(url).await {
            Ok(mut v) => {
                normalize_v2_to_v1(&mut v);
                return Ok(v);
            }
            Err(e) => debug!(error = %e, "[resolve] anon v2 failed"),
        }

        if let Some(token) = tokens.first() {
            let params = [("url".to_string(), url.to_string())];
            return self
                .sc
                .api_get_value_via_relay_proxy("/resolve", token, Some(&params))
                .await;
        }

        Err(direct_err)
    }

    async fn try_direct(&self, tokens: &[String], url: &str) -> AppResult<Value> {
        let params = [("url".to_string(), url.to_string())];
        let mut last_err: AppError = AppError::internal("no tokens for direct resolve");
        for token in tokens.iter().take(MAX_DIRECT_ATTEMPTS) {
            match self
                .sc
                .api_get_value_direct("/resolve", token, Some(&params))
                .await
            {
                Ok(v) => return Ok(v),
                Err(e) => {
                    debug!(error = %e, "[resolve] direct attempt failed");
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    async fn collect_direct_tokens(&self, user_token: Option<&str>) -> AppResult<Vec<String>> {
        let now = Utc::now().naive_utc();
        let limit = MAX_DIRECT_ATTEMPTS as i64;
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT access_token FROM sessions \
             WHERE access_token <> '' AND expires_at > $1 \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;

        let mut tokens: Vec<String> = Vec::with_capacity(MAX_DIRECT_ATTEMPTS);
        if let Some(t) = user_token {
            if !t.is_empty() {
                tokens.push(t.to_string());
            }
        }
        for (t,) in rows {
            if !tokens.iter().any(|x| x == &t) {
                tokens.push(t);
            }
        }
        Ok(tokens)
    }
}
