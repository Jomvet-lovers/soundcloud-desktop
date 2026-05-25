use std::sync::Arc;

use serde_json::Value;
use tracing::debug;

use super::anon::{normalize_v2_to_v1, AnonResolveClient};
use crate::error::{AppError, AppResult};
use crate::modules::auth::{TokenKind, TokenProvider};
use crate::sc::ScClient;

pub struct ResolveService {
    sc: ScClient,
    tokens: Arc<TokenProvider>,
    anon: AnonResolveClient,
}

impl ResolveService {
    pub fn new(sc: ScClient, tokens: Arc<TokenProvider>) -> Arc<Self> {
        let anon = AnonResolveClient::new(sc.clone());
        Arc::new(Self { sc, tokens, anon })
    }

    /// apiv2 /tracks/{id} через scraped anon client_id — для добычи реального
    /// `full_duration` cron'ом duration_resolver'а.
    pub async fn fetch_track_v2(&self, sc_track_id: &str) -> AppResult<Value> {
        self.anon.fetch_track(sc_track_id).await
    }

    /// `/resolve` для public URL'ов. `kind` определяет, откуда брать токены:
    /// `UserFirst(session)` если запрос инициировал юзер, `PublicPool` для cron'ов.
    /// На anti-bot 403 от SC падаем на анонимный apiv2 (`anon.resolve`); если и
    /// он не сработал — relay-proxy с одним из доступных токенов.
    pub async fn resolve(&self, kind: TokenKind, url: &str) -> AppResult<Value> {
        let chain = self.tokens.chain(kind).await?;
        let params = [("url".to_string(), url.to_string())];

        let mut last_err: AppError = AppError::internal("resolve: no tokens");
        for token in &chain {
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

        match self.anon.resolve(url).await {
            Ok(mut v) => {
                normalize_v2_to_v1(&mut v);
                return Ok(v);
            }
            Err(e) => debug!(error = %e, "[resolve] anon v2 failed"),
        }

        if let Some(token) = chain.first() {
            return self
                .sc
                .api_get_value_via_relay_proxy("/resolve", token, Some(&params))
                .await;
        }
        Err(last_err)
    }
}
