use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use bytes::Bytes;
use call_relay::{Client as RelayClient, Request as RelayRequest};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method};
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
    relay: Option<Arc<RelayClient>>,
}

#[derive(Clone, Copy)]
enum Channel {
    Direct,
    Proxy,
    Relay,
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
                relay: None,
            }),
        })
    }

    pub fn with_relay(self, relay: Arc<RelayClient>) -> Self {
        let inner = Arc::new(Inner {
            http: self.inner.http.clone(),
            proxy_url: self.inner.proxy_url.clone(),
            proxy_fallback: self.inner.proxy_fallback,
            observer: OnceCell::new(),
            relay: Some(relay),
        });
        if let Some(obs) = self.inner.observer.get().cloned() {
            let _ = inner.observer.set(obs);
        }
        Self { inner }
    }

    pub fn auth_base_url(&self) -> &str {
        AUTH_BASE
    }

    pub fn has_relay(&self) -> bool {
        self.inner.relay.is_some()
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

    /// Client Credentials grant: app-only токен под public-операции (search,
    /// resolve, public reads). Креды передаются ТОЛЬКО через HTTP Basic
    /// (Authorization: Basic Base64(client_id:client_secret)) — SC отвергает
    /// body-кредентиалы для этого grant'а. Возвращённый access_token переиспользуется
    /// в пуле; ratelimit: 50 токенов/12h/app, 30/1h/IP — управляется в
    /// OAuthAppTokenService.
    pub async fn exchange_client_credentials_for_token(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> AppResult<ScTokenResponse> {
        let body = serde_urlencoded::to_string([("grant_type", "client_credentials")])
            .map_err(|e| AppError::internal(format!("urlencode: {e}")))?;

        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("{client_id}:{client_secret}"));

        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Basic {basic}"))
                .map_err(|e| AppError::internal(format!("basic header: {e}")))?,
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

    /// GET по абсолютному URL (без префикса API_BASE). Используется для
    /// продолжения SC pagination через `next_href`, который приходит как
    /// полный URL — пересборка из cursor/offset роняла пагинацию плейлистов
    /// (SC ждёт `offset=`, а мы клали `cursor=`).
    pub async fn api_get_absolute_value(
        &self,
        absolute_url: &str,
        access_token: &str,
    ) -> AppResult<Value> {
        let headers = auth_headers(access_token, false);
        let bytes = self
            .with_fallback(Method::GET, absolute_url, headers, None, true)
            .await?;
        self.observe(&bytes, access_token);
        decode_json(&bytes)
    }

    pub async fn anon_get_via_relay_proxy(
        &self,
        target_url: &str,
        headers: HeaderMap,
    ) -> AppResult<Bytes> {
        self.race_relay_proxy(Method::GET, target_url, headers, None)
            .await
    }

    /// apiv2 `/resolve` run via the relay (the signed `sc.resolve_track` Lua method).
    /// Returns the raw apiv2 track JSON, or None when there is no relay / it is
    /// disabled / the relay couldn't resolve / the track was not found — the caller
    /// then falls back.
    pub async fn resolve_track_via_relay(&self, url: &str) -> Option<Value> {
        let relay = self.inner.relay.as_ref()?;
        let inputs = serde_json::to_vec(&serde_json::json!({ "url": url })).ok()?;
        let out = match relay
            .call_method(
                "sc.resolve_track",
                crate::sc::lua_methods::RESOLVE_TRACK,
                Bytes::from(inputs),
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                if !e.is_disabled() {
                    tracing::debug!(error = %e, "[resolve] relay sc.resolve_track failed");
                }
                return None;
            }
        };
        let parsed: Value = serde_json::from_slice(&out).ok()?;
        if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
            parsed.get("track").cloned()
        } else {
            None
        }
    }

    /// apiv2 `/users/{id}` run via the relay (the `sc.user_by_id` Lua method).
    /// Returns the RAW apiv2 user JSON, or None to fall back.
    pub async fn user_by_id_via_relay(&self, user_id: &str) -> Option<Value> {
        let relay = self.inner.relay.as_ref()?;
        let inputs = serde_json::to_vec(&serde_json::json!({ "id": user_id })).ok()?;
        let out = match relay
            .call_method(
                "sc.user_by_id",
                crate::sc::lua_methods::USER_BY_ID,
                Bytes::from(inputs),
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                if !e.is_disabled() {
                    tracing::debug!(error = %e, "[user_v2] relay sc.user_by_id failed");
                }
                return None;
            }
        };
        let parsed: Value = serde_json::from_slice(&out).ok()?;
        if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
            parsed.get("user").cloned()
        } else {
            None
        }
    }

    /// apiv2 `/tracks/{id}` run via the relay (the `sc.track_by_id` Lua method).
    /// Returns the RAW apiv2 track JSON (not v1-normalized), or None to fall back.
    pub async fn track_by_id_via_relay(&self, sc_track_id: &str) -> Option<Value> {
        let relay = self.inner.relay.as_ref()?;
        let inputs = serde_json::to_vec(&serde_json::json!({ "id": sc_track_id })).ok()?;
        let out = match relay
            .call_method(
                "sc.track_by_id",
                crate::sc::lua_methods::TRACK_BY_ID,
                Bytes::from(inputs),
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                if !e.is_disabled() {
                    tracing::debug!(error = %e, "[track_v2] relay sc.track_by_id failed");
                }
                return None;
            }
        };
        let parsed: Value = serde_json::from_slice(&out).ok()?;
        if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
            parsed.get("track").cloned()
        } else {
            None
        }
    }

    /// apiv2 playlist (+ full ordered tracks when `hydrate`) via the relay (the signed
    /// `sc.playlist_full` Lua method). Returns the RAW apiv2 playlist, or None to fall back.
    pub async fn playlist_full_via_relay(&self, playlist_id: &str, hydrate: bool) -> Option<Value> {
        let inputs =
            serde_json::to_vec(&serde_json::json!({ "id": playlist_id, "hydrate": hydrate }))
                .ok()?;
        let v = self
            .call_relay_method(
                "sc.playlist_full",
                crate::sc::lua_methods::PLAYLIST_FULL,
                inputs,
            )
            .await?;
        (v.get("ok").and_then(Value::as_bool) == Some(true))
            .then(|| v.get("playlist").cloned())
            .flatten()
    }

    /// apiv2 one page of a public per-user collection via the relay (`sc.user_collection`).
    /// Returns the RAW `{ collection, next_href }` response, or None to fall back.
    pub async fn user_collection_via_relay(
        &self,
        user_id: &str,
        kind: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> Option<Value> {
        let inputs = serde_json::to_vec(&serde_json::json!({
            "user_id": user_id, "kind": kind, "cursor": cursor, "limit": limit,
        }))
        .ok()?;
        let v = self
            .call_relay_method(
                "sc.user_collection",
                crate::sc::lua_methods::USER_COLLECTION,
                inputs,
            )
            .await?;
        (v.get("ok").and_then(Value::as_bool) == Some(true)).then_some(v)
    }

    /// apiv2 one page of a typed search via the relay (`sc.search`). Returns the RAW
    /// `{ collection, next_href, total_results }` response, or None to fall back.
    pub async fn search_via_relay(
        &self,
        search_type: &str,
        q: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> Option<Value> {
        let inputs = serde_json::to_vec(&serde_json::json!({
            "type": search_type, "q": q, "cursor": cursor, "limit": limit,
        }))
        .ok()?;
        let v = self
            .call_relay_method("sc.search", crate::sc::lua_methods::SEARCH, inputs)
            .await?;
        (v.get("ok").and_then(Value::as_bool) == Some(true)).then_some(v)
    }

    /// Run a signed Lua method via the relay, returning its parsed JSON output or None
    /// (no relay / disabled / transport error / bad JSON — the caller falls back).
    async fn call_relay_method(
        &self,
        method_id: &'static str,
        script: &'static str,
        inputs: Vec<u8>,
    ) -> Option<Value> {
        let relay = self.inner.relay.as_ref()?;
        let out = match relay
            .call_method(method_id, script, Bytes::from(inputs))
            .await
        {
            Ok(b) => b,
            Err(e) => {
                if !e.is_disabled() {
                    tracing::debug!(error = %e, method = method_id, "relay lua method failed");
                }
                return None;
            }
        };
        serde_json::from_slice(&out).ok()
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
        let proxy_set = !self.inner.proxy_url.is_empty();
        let relay_set = self.inner.relay.is_some();
        let is_get = method == Method::GET;
        let pf = self.inner.proxy_fallback;

        if !proxy_set && !relay_set {
            return self.send_direct(method, target_url, headers, body).await;
        }

        if !api_call {
            // auth: direct → proxy → relay
            return self
                .try_chain(
                    method,
                    target_url,
                    headers,
                    body,
                    &[Channel::Direct, Channel::Proxy, Channel::Relay],
                )
                .await;
        }

        if pf {
            if is_get {
                // direct → race(relay, proxy)
                match self
                    .send_direct(method.clone(), target_url, headers.clone(), body.clone())
                    .await
                {
                    Ok(b) => return Ok(b),
                    Err(e) => tracing::debug!(error = %e, "direct failed, racing relay&proxy"),
                }
                self.race_relay_proxy(method, target_url, headers, body)
                    .await
            } else {
                self.try_chain(
                    method,
                    target_url,
                    headers,
                    body,
                    &[Channel::Direct, Channel::Proxy, Channel::Relay],
                )
                .await
            }
        } else if is_get {
            self.race_relay_proxy(method, target_url, headers, body)
                .await
        } else {
            self.try_chain(
                method,
                target_url,
                headers,
                body,
                &[Channel::Proxy, Channel::Relay],
            )
            .await
        }
    }

    async fn try_chain(
        &self,
        method: Method,
        target_url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
        chain: &[Channel],
    ) -> AppResult<Bytes> {
        let mut last: Option<AppError> = None;
        for ch in chain {
            let r = match ch {
                Channel::Direct => {
                    self.send_direct(method.clone(), target_url, headers.clone(), body.clone())
                        .await
                }
                Channel::Proxy => {
                    if self.inner.proxy_url.is_empty() {
                        continue;
                    }
                    self.send_proxy(method.clone(), target_url, headers.clone(), body.clone())
                        .await
                }
                Channel::Relay => {
                    if self.inner.relay.is_none() {
                        continue;
                    }
                    self.send_relay(
                        method.clone(),
                        target_url.to_string(),
                        headers.clone(),
                        body.clone(),
                    )
                    .await
                }
            };
            match r {
                Ok(b) => return Ok(b),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| AppError::internal("no channels available")))
    }

    async fn race_relay_proxy(
        &self,
        method: Method,
        target_url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        let proxy_set = !self.inner.proxy_url.is_empty();
        let relay_set = self.inner.relay.is_some();
        match (relay_set, proxy_set) {
            (true, true) => {
                let m1 = method.clone();
                let u1 = target_url.to_string();
                let h1 = headers.clone();
                let b1 = body.clone();
                let relay_fut: std::pin::Pin<
                    Box<dyn std::future::Future<Output = AppResult<Bytes>> + Send + '_>,
                > = Box::pin(self.send_relay(m1, u1, h1, b1));
                let proxy_fut: std::pin::Pin<
                    Box<dyn std::future::Future<Output = AppResult<Bytes>> + Send + '_>,
                > = Box::pin(self.send_proxy(method, target_url, headers, body));
                match futures::future::select_ok(vec![relay_fut, proxy_fut]).await {
                    Ok((b, _)) => Ok(b),
                    Err(e) => Err(e),
                }
            }
            (true, false) => {
                self.send_relay(method, target_url.to_string(), headers, body)
                    .await
            }
            (false, true) => self.send_proxy(method, target_url, headers, body).await,
            (false, false) => self.send_direct(method, target_url, headers, body).await,
        }
    }

    async fn send_direct(
        &self,
        method: Method,
        target_url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        self.send(method, target_url, headers, body, false).await
    }

    async fn send_proxy(
        &self,
        method: Method,
        target_url: &str,
        headers: HeaderMap,
        body: Option<Bytes>,
    ) -> AppResult<Bytes> {
        self.send(method, target_url, headers, body, true).await
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
            let v: Value = if resp.body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&resp.body).unwrap_or_else(|_| {
                    Value::String(String::from_utf8_lossy(&resp.body).into_owned())
                })
            };
            return Err(AppError::ScApi {
                status: resp.status,
                body: v,
            });
        }
        Ok(resp.body)
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
