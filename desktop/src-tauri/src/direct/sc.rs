//! Minimal SoundCloud api-v2 client for direct mode.
//!
//! Reads only. The public `client_id` is extracted from the SoundCloud
//! homepage hydration (same trick the track cache uses) and cached with a
//! refresh-on-failure policy. User-scoped calls attach the `oauth_token`
//! arriving from the frontend as an `Authorization: OAuth ...` header, which
//! is the form SoundCloud expects.

use std::time::{Duration, Instant};

use serde_json::Value;

use super::DirectState;

pub const SC_API: &str = "https://api-v2.soundcloud.com";
pub const SC_HOME: &str = "https://soundcloud.com";
const UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// `soundcloud:tracks:123` -> `123`; a bare id passes through.
pub fn id_of(urn: &str) -> String {
    urn.rsplit(':').next().unwrap_or(urn).to_string()
}

impl DirectState {
    pub async fn client_id(&self) -> Result<String, String> {
        {
            let guard = self.client_id.lock().await;
            if let Some(cid) = guard.as_ref() {
                if !cid.is_empty() {
                    return Ok(cid.clone());
                }
            }
        }
        let fresh = fetch_client_id(&self.http).await?;
        *self.client_id.lock().await = Some(fresh.clone());
        Ok(fresh)
    }

    /// GET a SoundCloud api-v2 path (may include a query string). `client_id`
    /// is appended automatically. Returns (status, json).
    pub async fn sc_get(
        &self,
        path_query: &str,
        token: Option<&str>,
    ) -> Result<(u16, Value), String> {
        let cid = self.client_id().await?;
        let url = with_client_id(path_query, &cid);

        let resp = send_get(&self.http, &url, token).await?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|e| e.to_string())?;

        // A stale client_id shows up as 401/403 on otherwise public calls.
        if status == 401 || status == 403 {
            if let Ok(fresh) = fetch_client_id(&self.http).await {
                *self.client_id.lock().await = Some(fresh.clone());
                if let Ok(resp) = send_get(&self.http, &with_client_id(path_query, &fresh), token).await
                {
                    let st = resp.status().as_u16();
                    let body = resp.text().await.unwrap_or_default();
                    let json = serde_json::from_str(&body).unwrap_or(Value::Null);
                    return Ok((st, json));
                }
            }
        }

        let json = serde_json::from_str(&body).unwrap_or(Value::Null);
        Ok((status, json))
    }

    /// GET a path and map (401/404/empty) into `None`; other statuses return
    /// the JSON body. Used for optional enrichments.
    pub async fn sc_get_opt(&self, path_query: &str, token: Option<&str>) -> Option<Value> {
        match self.sc_get(path_query, token).await {
            Ok((s, v)) if (200..300).contains(&s) => Some(v),
            _ => None,
        }
    }

    /// Cached SoundCloud user id of the current token (`/me`).
    pub async fn my_user_id(&self, token: &str) -> Result<u64, String> {
        {
            let guard = self.me_cache.lock().await;
            if let Some(c) = guard.as_ref() {
                if c.at.elapsed() < Duration::from_secs(600) {
                    return Ok(c.user_id);
                }
            }
        }
        let me = fetch_me(self, token).await?;
        let id = me
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| "no id in /me".to_string())?;
        *self.me_cache.lock().await = Some(MeCache {
            user_id: id,
            at: Instant::now(),
        });
        Ok(id)
    }
}

fn with_client_id(path_query: &str, cid: &str) -> String {
    let sep = if path_query.contains('?') { '&' } else { '?' };
    format!("{SC_API}{path_query}{sep}client_id={cid}")
}

async fn send_get(
    http: &wreq::Client,
    url: &str,
    token: Option<&str>,
) -> Result<wreq::Response, String> {
    let mut req = http
        .get(url)
        .header("User-Agent", UA)
        .header("Accept", "application/json, text/javascript, */*; q=0.1")
        .timeout(Duration::from_secs(30));
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        req = req.header("Authorization", format!("OAuth {t}"));
    }
    req.send().await.map_err(|e| e.to_string())
}

pub async fn fetch_client_id(http: &wreq::Client) -> Result<String, String> {
    let resp = http
        .get(SC_HOME)
        .header("User-Agent", UA)
        .header("Accept", "text/html,application/xhtml+xml")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("fetch sc home: {e}"))?;
    let html = resp
        .text()
        .await
        .map_err(|e| format!("read sc home: {e}"))?;

    static PATTERN: &str =
        r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([^"]+)""#;
    let re = regex::Regex::new(PATTERN).map_err(|e| e.to_string())?;
    re.captures(&html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| "client_id not found in SoundCloud homepage".to_string())
}

pub async fn fetch_me(state: &DirectState, token: &str) -> Result<Value, String> {
    let (status, body) = state.sc_get("/me", Some(token)).await?;
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        Err(format!("SoundCloud /me answered {status}"))
    }
}

/// Cache entry for `/me`: (user_id, fetched_at).
pub struct MeCache {
    pub user_id: u64,
    pub at: Instant,
}
