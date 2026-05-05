use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::OnceCell;

use crate::config::SoundcloudCfg;
use crate::error::{AppError, AppResult};
use crate::sc::types::ScTokenResponse;

const API_BASE: &str = "https://api.soundcloud.com";
const AUTH_BASE: &str = "https://secure.soundcloud.com";

#[derive(Clone, Debug)]
pub struct OAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub trait TrackObserver: Send + Sync {
    fn observe(&self, body: Bytes, access_token: String);
}

#[derive(Clone)]
pub struct ScClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: Client,
    proxy_url: String,
    proxy_fallback: bool,
    observer: OnceCell<Arc<dyn TrackObserver>>,
}

impl ScClient {
    pub fn new(cfg: &SoundcloudCfg) -> Result<Self, reqwest::Error> {
        let http = Client::builder()
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(20)
            .pool_idle_timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .user_agent("scd-backend/0.1")
            .build()?;

        Ok(Self {
            inner: Arc::new(Inner {
                http,
                proxy_url: cfg.proxy_url.clone(),
                proxy_fallback: cfg.proxy_fallback,
                observer: OnceCell::new(),
            }),
        })
    }

    pub fn auth_base_url(&self) -> &str {
        AUTH_BASE
    }

    pub fn http(&self) -> &Client {
        &self.inner.http
    }

    pub fn install_track_observer(&self, obs: Arc<dyn TrackObserver>) {
        let _ = self.inner.observer.set(obs);
    }

    pub async fn exchange_code_for_token(
        &self,
        code: &str,
        code_verifier: &str,
        creds: &OAuthCredentials,
    ) -> AppResult<ScTokenResponse> {
        let body = serde_urlencoded::to_string([
            ("grant_type", "authorization_code"),
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", creds.redirect_uri.as_str()),
            ("code_verifier", code_verifier),
        ])
        .map_err(|e| AppError::internal(format!("urlencode: {e}")))?;

        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );

        let url = format!("{AUTH_BASE}/oauth/token");
        let bytes = self
            .with_fallback(Method::POST, &url, headers, Some(Bytes::from(body)), false)
            .await?;
        decode_json(&bytes)
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: &str,
        creds: &OAuthCredentials,
    ) -> AppResult<ScTokenResponse> {
        let body = serde_urlencoded::to_string([
            ("grant_type", "refresh_token"),
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("refresh_token", refresh_token),
        ])
        .map_err(|e| AppError::internal(format!("urlencode: {e}")))?;

        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );

        let url = format!("{AUTH_BASE}/oauth/token");
        let bytes = self
            .with_fallback(Method::POST, &url, headers, Some(Bytes::from(body)), false)
            .await?;
        decode_json(&bytes)
    }

    pub async fn sign_out(&self, access_token: &str) {
        let body = serde_json::json!({ "access_token": access_token }).to_string();
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );

        let url = format!("{AUTH_BASE}/sign-out");
        if let Err(e) = self
            .with_fallback(Method::POST, &url, headers, Some(Bytes::from(body)), false)
            .await
        {
            tracing::debug!(error = %e, "sign-out call failed (ignored)");
        }
    }

    pub async fn api_get<T: DeserializeOwned>(
        &self,
        path: &str,
        access_token: &str,
        params: Option<&[(String, String)]>,
    ) -> AppResult<T> {
        let url = build_api_url(path, params);
        let headers = auth_headers(access_token, false);
        let bytes = self
            .with_fallback(Method::GET, &url, headers, None, true)
            .await?;
        self.observe(&bytes, access_token);
        decode_json(&bytes)
    }

    pub async fn api_get_value(
        &self,
        path: &str,
        access_token: &str,
        params: Option<&[(String, String)]>,
    ) -> AppResult<Value> {
        self.api_get::<Value>(path, access_token, params).await
    }

    pub async fn api_post<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        access_token: &str,
        body: Option<&B>,
    ) -> AppResult<T> {
        let url = format!("{API_BASE}{path}");
        let headers = auth_headers(access_token, true);
        let payload = match body {
            Some(b) => Bytes::from(
                serde_json::to_vec(b)
                    .map_err(|e| AppError::internal(format!("json encode: {e}")))?,
            ),
            None => Bytes::new(),
        };
        let bytes = self
            .with_fallback(Method::POST, &url, headers, Some(payload), true)
            .await?;
        self.observe(&bytes, access_token);
        decode_json(&bytes)
    }

    pub async fn api_post_value(
        &self,
        path: &str,
        access_token: &str,
        body: Option<&Value>,
    ) -> AppResult<Value> {
        self.api_post::<Value, Value>(path, access_token, body)
            .await
    }

    pub async fn api_put<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        access_token: &str,
        body: Option<&B>,
    ) -> AppResult<T> {
        let url = format!("{API_BASE}{path}");
        let headers = auth_headers(access_token, true);
        let payload = match body {
            Some(b) => Bytes::from(
                serde_json::to_vec(b)
                    .map_err(|e| AppError::internal(format!("json encode: {e}")))?,
            ),
            None => Bytes::new(),
        };
        let bytes = self
            .with_fallback(Method::PUT, &url, headers, Some(payload), true)
            .await?;
        self.observe(&bytes, access_token);
        decode_json(&bytes)
    }

    pub async fn api_put_value(
        &self,
        path: &str,
        access_token: &str,
        body: Option<&Value>,
    ) -> AppResult<Value> {
        self.api_put::<Value, Value>(path, access_token, body).await
    }

    pub async fn api_delete(&self, path: &str, access_token: &str) -> AppResult<Value> {
        let url = format!("{API_BASE}{path}");
        let headers = auth_headers(access_token, false);
        let bytes = self
            .with_fallback(Method::DELETE, &url, headers, None, true)
            .await?;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        self.observe(&bytes, access_token);
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => Ok(v),
            Err(_) => Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned())),
        }
    }

    fn observe(&self, bytes: &Bytes, access_token: &str) {
        if access_token.is_empty() || bytes.is_empty() {
            return;
        }
        if let Some(obs) = self.inner.observer.get() {
            obs.observe(bytes.clone(), access_token.to_string());
        }
    }

    async fn with_fallback(
        &self,
        method: Method,
        target_url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
        api_call: bool,
    ) -> AppResult<Bytes> {
        let proxy = &self.inner.proxy_url;

        if proxy.is_empty() || !api_call {
            return self.send(method, target_url, headers, body, false).await;
        }

        if self.inner.proxy_fallback {
            match self
                .send(
                    method.clone(),
                    target_url,
                    headers.clone(),
                    body.clone(),
                    false,
                )
                .await
            {
                Ok(r) => return Ok(r),
                Err(direct_err) => {
                    tracing::debug!(error = %direct_err, "Direct call failed, falling back to proxy");
                    return self.send(method, target_url, headers, body, true).await;
                }
            }
        }

        self.send(method, target_url, headers, body, true).await
    }

    async fn send(
        &self,
        method: Method,
        target_url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
        via_proxy: bool,
    ) -> AppResult<Bytes> {
        let (url, mut extra_headers) = if via_proxy && !self.inner.proxy_url.is_empty() {
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
            let body: Value = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes)
                    .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
            };
            return Err(AppError::ScApi {
                status: status.as_u16(),
                body,
            });
        }

        Ok(bytes)
    }
}

fn auth_headers(access_token: &str, with_content_type: bool) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&format!("OAuth {access_token}")) {
        h.insert(AUTHORIZATION, v);
    }
    h.insert(
        ACCEPT,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    if with_content_type {
        h.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    h
}

fn build_api_url(path: &str, params: Option<&[(String, String)]>) -> String {
    let base = format!("{API_BASE}{path}");
    match params {
        Some(p) if !p.is_empty() => {
            let qs = serde_urlencoded::to_string(p).unwrap_or_default();
            if qs.is_empty() {
                base
            } else {
                format!("{base}?{qs}")
            }
        }
        _ => base,
    }
}

fn decode_json<T: DeserializeOwned>(bytes: &Bytes) -> AppResult<T> {
    if bytes.is_empty() {
        return serde_json::from_slice::<T>(b"null")
            .map_err(|e| AppError::internal(format!("empty body decode: {e}")));
    }
    serde_json::from_slice(bytes).map_err(|e| {
        tracing::warn!(error = %e, "SC JSON decode failed");
        AppError::internal(format!("SC JSON decode: {e}"))
    })
}
