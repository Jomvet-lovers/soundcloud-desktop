//! Универсальный fetcher для внешних API (Genius/MusicBrainz/Wikipedia/...).
//!
//! Режимы:
//! - `get_bytes`  — direct → `proxy_first`. Без throttle, без ретраев на direct.
//! - `get_api`    — throttle → direct → `proxy_first`. Для API с токеном.
//! - `get_scrape` — `proxy_first` → fallback throttle→direct. Для web без токена.
//!
//! `proxy_first` — прокси primary с N ретраями (intermediate-proxy round-robin'ит
//! пул egress-IP per-request), релей — единственный capped last-resort shot, НЕ
//! racing'ом: scarce-релей (реальные десктопы) не контендится и под глобальным
//! семафором, так что аутейдж прокси не зальёт его флудом.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use call_relay::{Client as RelayClient, Request as RelayRequest};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use tokio::sync::Semaphore;
use tracing::debug;

use crate::common::throttle::Throttle;
use crate::error::{AppError, AppResult};

const PROXY_RETRY_ATTEMPTS: u32 = 4;
const PROXY_RETRY_BASE_MS: u64 = 300;
const RELAY_MAX_CONCURRENT: usize = 8;

#[derive(Clone)]
pub struct ExternalFetcher {
    inner: Arc<Inner>,
}

struct Inner {
    http: Client,
    proxy_url: String,
    relay: Option<Arc<RelayClient>>,
    relay_sem: Arc<Semaphore>,
}

impl ExternalFetcher {
    pub fn new(http: Client, proxy_url: String, relay: Option<Arc<RelayClient>>) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Inner {
                http,
                proxy_url,
                relay,
                relay_sem: Arc::new(Semaphore::new(RELAY_MAX_CONCURRENT)),
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
                self.proxy_first(Method::GET, url, headers, None).await
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

    pub async fn get_scrape(
        &self,
        url: &str,
        headers: HeaderMap,
        throttle: &Throttle,
    ) -> AppResult<Bytes> {
        if self.has_fallback() {
            match self
                .proxy_first(Method::GET, url, headers.clone(), None)
                .await
            {
                Ok(b) => return Ok(b),
                Err(e) => debug!(url, error = %e, "scrape proxy_first failed; falling to direct"),
            }
        }
        throttle.wait().await;
        self.send_direct(Method::GET, url, headers, None).await
    }

    pub async fn proxy_first(
        &self,
        method: Method,
        url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        let mut last_err: Option<AppError> = None;
        if !self.inner.proxy_url.is_empty() {
            for attempt in 0..PROXY_RETRY_ATTEMPTS {
                match self
                    .send_proxy(method.clone(), url, headers.clone(), body.clone())
                    .await
                {
                    Ok(b) => return Ok(b),
                    Err(e) => {
                        if is_hard_client_error(&e) {
                            return Err(e);
                        }
                        last_err = Some(e);
                        if attempt + 1 < PROXY_RETRY_ATTEMPTS {
                            let delay_ms = PROXY_RETRY_BASE_MS * (1u64 << attempt);
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        }
                    }
                }
            }
        }
        if self.inner.relay.is_some() {
            if let Ok(_permit) = self.inner.relay_sem.clone().try_acquire_owned() {
                match self.send_relay(method, url.to_string(), headers, body).await {
                    Ok(b) => return Ok(b),
                    Err(e) => last_err = last_err.or(Some(e)),
                }
            } else {
                debug!(url, "relay last-resort skipped: concurrency cap reached");
            }
        }
        Err(last_err
            .unwrap_or_else(|| AppError::ScUnreachable("no proxy or relay configured".to_string())))
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

        // proxy strips response content-encoding without decompressing → force identity
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

fn is_hard_client_error(e: &AppError) -> bool {
    matches!(
        e,
        AppError::ScApi { status, .. }
            if (400..500).contains(status) && !matches!(status, 429 | 408 | 425)
    )
}
