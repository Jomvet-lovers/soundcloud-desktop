use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tracing::debug;

use super::anon::{normalize_v2_to_v1, AnonResolveClient};
use crate::error::{AppError, AppResult};
use crate::modules::auth::{TokenKind, TokenProvider};
use crate::sc::{hedge, race, ChannelHealth, FetchStrategy, ScClient};

/// How long the relay primary gets a head start before the proxy/token backup is
/// hedged in. Long enough that a healthy relay almost always answers alone (1x SC
/// load), short enough that a stalled relay doesn't hold up the request.
const HEDGE_DELAY: Duration = Duration::from_millis(700);

pub struct ResolveService {
    sc: ScClient,
    tokens: Arc<TokenProvider>,
    anon: AnonResolveClient,
    lua_health: ChannelHealth,
    strategy: FetchStrategy,
}

impl ResolveService {
    pub fn new(sc: ScClient, tokens: Arc<TokenProvider>) -> Arc<Self> {
        let anon = AnonResolveClient::new(sc.clone());
        Arc::new(Self {
            sc,
            tokens,
            anon,
            lua_health: ChannelHealth::default(),
            strategy: FetchStrategy::from_env(),
        })
    }

    /// apiv2 /tracks/{id} — recovers the real `full_duration` for the
    /// duration_resolver cron. Tries the relay first, then the anon scrape.
    pub async fn fetch_track_v2(&self, sc_track_id: &str) -> AppResult<Value> {
        if self.sc.has_relay() && !self.lua_health.is_open() {
            if let Some(v) = self.sc.track_by_id_via_relay(sc_track_id).await {
                self.lua_health.record_ok();
                return Ok(v);
            }
            self.lua_health.record_ban();
        }
        self.anon.fetch_track(sc_track_id).await
    }

    /// `/resolve` for public URLs.
    ///
    /// Channel A (primary): the relay. Channel B (backup): the OAuth token chain →
    /// anon apiv2 scrape → relay-proxy. How the two combine (`hedge`/`race`/
    /// `fallback`) is set by `CALL_FETCH_STRATEGY`; the default `hedge` only starts
    /// the backup when the relay is slow or down, so it stays ~1x load on SC. After a
    /// run of failures channel A's breaker routes straight to B.
    pub async fn resolve(&self, kind: TokenKind, url: &str) -> AppResult<Value> {
        let relay_usable = self.sc.has_relay() && !self.lua_health.is_open();
        if !relay_usable {
            return self.resolve_via_chain(kind, url).await;
        }

        match self.strategy {
            FetchStrategy::Fallback => match self.resolve_via_lua(url).await {
                Ok(v) => Ok(v),
                Err(_) => self.resolve_via_chain(kind, url).await,
            },
            FetchStrategy::Hedge => {
                hedge(
                    self.resolve_via_lua(url),
                    HEDGE_DELAY,
                    self.resolve_via_chain(kind, url),
                )
                .await
            }
            FetchStrategy::Race => {
                race(self.resolve_via_lua(url), self.resolve_via_chain(kind, url)).await
            }
        }
    }

    /// Channel A — the signed `sc.resolve_track` Lua method run via the relay.
    async fn resolve_via_lua(&self, url: &str) -> AppResult<Value> {
        match self.sc.resolve_track_via_relay(url).await {
            Some(mut v) => {
                normalize_v2_to_v1(&mut v);
                self.lua_health.record_ok();
                Ok(v)
            }
            None => {
                // No result = the relay couldn't resolve (no client available / not
                // found). Treat as a failure for the breaker so a sustained relay
                // outage routes to B.
                self.lua_health.record_ban();
                Err(AppError::ScUnreachable(
                    "relay sc.resolve_track: no result".into(),
                ))
            }
        }
    }

    /// Channel B — OAuth token-direct → anon apiv2 scrape → relay-proxy with a token.
    async fn resolve_via_chain(&self, kind: TokenKind, url: &str) -> AppResult<Value> {
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
