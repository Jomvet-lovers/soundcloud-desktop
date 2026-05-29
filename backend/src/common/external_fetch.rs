//! Универсальный fetcher для внешних API (Genius/MusicBrainz/Wikipedia/...).
//!
//! Три режима:
//! - `get_bytes`  — direct → race(proxy, relay). Без throttle, без ретраев.
//! - `get_api`    — throttle → direct → race(proxy, relay). Для API с токеном.
//! - `get_scrape` — race(proxy, relay) с внутренним 429-retry → fallback direct
//!   (после throttle, без ретраев). Для HTML/web-API без токена, где direct
//!   режется CF/rate-limit'ом.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use call_relay::{Client as RelayClient, Request as RelayRequest};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use tracing::debug;

use crate::common::throttle::Throttle;
use crate::error::{AppError, AppResult};

const SCRAPE_RELAY_RETRIES: u32 = 3;
const SCRAPE_RETRY_BASE_MS: u64 = 800;

const RACE_BOUNDED_GRACE: Duration = Duration::from_secs(8);

#[derive(Clone)]
pub struct ExternalFetcher {
    inner: Arc<Inner>,
}

struct Inner {
    http: Client,
    proxy_url: String,
    relay: Option<Arc<RelayClient>>,
}

impl ExternalFetcher {
    pub fn new(http: Client, proxy_url: String, relay: Option<Arc<RelayClient>>) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Inner {
                http,
                proxy_url,
                relay,
            }),
        })
    }

    pub fn has_fallback(&self) -> bool {
        !self.inner.proxy_url.is_empty() || self.inner.relay.is_some()
    }

    /// Direct → race(proxy, relay) на ошибке. Без throttle, без ретраев.
    pub async fn get_bytes(&self, url: &str, headers: HeaderMap) -> AppResult<Bytes> {
        match self
            .send_direct(Method::GET, url, headers.clone(), None)
            .await
        {
            Ok(b) => Ok(b),
            Err(e) => {
                debug!(url, error = %e, "external direct failed, falling back");
                self.race_relay_proxy(Method::GET, url, headers, None).await
            }
        }
    }

    /// API-режим: throttle → direct → race(proxy, relay). Без ретраев.
    pub async fn get_api(
        &self,
        url: &str,
        headers: HeaderMap,
        throttle: &Throttle,
    ) -> AppResult<Bytes> {
        throttle.wait().await;
        self.get_bytes(url, headers).await
    }

    /// Scrape-режим: сначала race(proxy, relay) с внутренним ретраем на 429.
    /// При исчерпании — throttle → direct (без ретраев).
    /// Если fallback не настроен — сразу throttle → direct.
    pub async fn get_scrape(
        &self,
        url: &str,
        headers: HeaderMap,
        throttle: &Throttle,
    ) -> AppResult<Bytes> {
        if self.has_fallback() {
            for attempt in 0..=SCRAPE_RELAY_RETRIES {
                match self
                    .race_relay_proxy(Method::GET, url, headers.clone(), None)
                    .await
                {
                    Ok(b) => return Ok(b),
                    Err(e) => {
                        let retryable = matches!(
                            &e,
                            AppError::ScApi { status, .. }
                                if *status == 429 || *status == 408 || *status == 425
                        ) || matches!(&e, AppError::ScUnreachable(_));
                        if attempt < SCRAPE_RELAY_RETRIES && retryable {
                            let delay_ms = SCRAPE_RETRY_BASE_MS * (1u64 << attempt);
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                            continue;
                        }
                        debug!(url, attempt, error = %e, "scrape relay/proxy failed; falling to direct");
                        break;
                    }
                }
            }
        }
        throttle.wait().await;
        self.send_direct(Method::GET, url, headers, None).await
    }

    pub async fn race_relay_proxy(
        &self,
        method: Method,
        url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        let proxy_set = !self.inner.proxy_url.is_empty();
        let relay_set = self.inner.relay.is_some();
        match (relay_set, proxy_set) {
            (true, true) => {
                let relay_fut = self.send_relay(
                    method.clone(),
                    url.to_string(),
                    headers.clone(),
                    body.clone(),
                );
                let proxy_fut = self.send_proxy(method, url, headers, body);
                tokio::pin!(relay_fut);
                tokio::pin!(proxy_fut);

                tokio::select! {
                    relay_res = relay_fut.as_mut() => match relay_res {
                        Ok(b) => Ok(b),
                        Err(relay_err) => {
                            await_other_with_grace(proxy_fut, RACE_BOUNDED_GRACE, relay_err).await
                        }
                    },
                    proxy_res = proxy_fut.as_mut() => match proxy_res {
                        Ok(b) => Ok(b),
                        Err(proxy_err) => {
                            let unbounded = matches!(
                                &proxy_err,
                                AppError::ScApi { status: 502, .. },
                            );
                            if unbounded {
                                match relay_fut.await {
                                    Ok(b) => Ok(b),
                                    Err(_) => Err(proxy_err),
                                }
                            } else {
                                await_other_with_grace(relay_fut, RACE_BOUNDED_GRACE, proxy_err).await
                            }
                        }
                    },
                }
            }
            (true, false) => {
                self.send_relay(method, url.to_string(), headers, body)
                    .await
            }
            (false, true) => self.send_proxy(method, url, headers, body).await,
            (false, false) => Err(AppError::ScUnreachable(
                "no relay or proxy configured".to_string(),
            )),
        }
    }

    async fn send_direct(
        &self,
        method: Method,
        url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        self.send(method, url, headers, body, false).await
    }

    async fn send_proxy(
        &self,
        method: Method,
        url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        if self.inner.proxy_url.is_empty() {
            return Err(AppError::internal("proxy not configured"));
        }
        self.send(method, url, headers, body, true).await
    }

    async fn send(
        &self,
        method: Method,
        target_url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
        via_proxy: bool,
    ) -> AppResult<Bytes> {
        let (url, mut extra_headers) = if via_proxy {
            let encoded = base64::engine::general_purpose::STANDARD.encode(target_url);
            let mut h = headers;
            h.insert(
                HeaderName::from_static("x-target"),
                HeaderValue::from_str(&encoded)
                    .map_err(|e| AppError::internal(format!("bad x-target: {e}")))?,
            );
            (self.inner.proxy_url.clone(), h)
        } else {
            (target_url.to_string(), headers)
        };

        // Never let the upstream compress: the scraping proxy forwards bodies
        // as-is and strips `content-encoding`, so a gzip/br response would reach
        // us undecodable (this silently killed Genius once). Asking for
        // `identity` guarantees a plain body whether we go direct or via proxy.
        extra_headers.insert(
            reqwest::header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );

        let mut builder = self.inner.http.request(method, &url);
        for (k, v) in extra_headers.drain() {
            if let Some(name) = k {
                builder = builder.header(name, v);
            }
        }
        if let Some(b) = body {
            builder = builder.body(b);
        }
        let resp = builder
            .send()
            .await
            .map_err(|e| AppError::ScUnreachable(e.to_string()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::ScUnreachable(e.to_string()))?;
        if status.is_client_error() || status.is_server_error() {
            return Err(AppError::ScApi {
                status: status.as_u16(),
                body: serde_json::Value::String(
                    String::from_utf8_lossy(&bytes).chars().take(200).collect(),
                ),
            });
        }
        Ok(bytes)
    }

    async fn send_relay(
        &self,
        method: Method,
        target_url: String,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        let relay = self
            .inner
            .relay
            .as_ref()
            .ok_or_else(|| AppError::internal("relay not configured"))?;
        let mut h: HashMap<String, String> = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(vs) = v.to_str() {
                h.insert(k.as_str().to_string(), vs.to_string());
            }
        }
        let req = RelayRequest {
            url: target_url,
            method: method.as_str().to_string(),
            headers: h,
            body: body.unwrap_or_default(),
        };
        let resp = relay
            .fetch(&req)
            .await
            .map_err(|e| AppError::ScUnreachable(e.to_string()))?;
        if resp.status >= 400 {
            return Err(AppError::ScApi {
                status: resp.status,
                body: serde_json::Value::String(
                    String::from_utf8_lossy(&resp.body)
                        .chars()
                        .take(200)
                        .collect(),
                ),
            });
        }
        Ok(resp.body)
    }
}

async fn await_other_with_grace<F>(
    other: F,
    grace: Duration,
    original_err: AppError,
) -> AppResult<Bytes>
where
    F: std::future::Future<Output = AppResult<Bytes>>,
{
    match tokio::time::timeout(grace, other).await {
        Ok(Ok(b)) => Ok(b),
        Ok(Err(_)) | Err(_) => Err(original_err),
    }
}
