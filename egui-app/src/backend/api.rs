//! Phase 3: ローカル warp API (`direct/routes`) への thin client。
//! 対応: `desktop/src/lib/api-client.ts` の direct-mode 部分。
//! 全トラフィックはアプリ内サーバ (`http://127.0.0.1:{api_port}`) 向き。
//! host-status / premium / star フェイルオーバは direct-mode では不要
//! (全 base が同一ローカルに解決されるため) ので持たない。

use std::time::Duration;

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct ApiClient {
    base: String,
    session: Option<String>,
    client: wreq::Client,
}

impl ApiClient {
    pub fn new(api_port: u16) -> Self {
        Self {
            base: format!("http://127.0.0.1:{api_port}"),
            session: None,
            client: wreq::Client::new(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn session_token(&self) -> Option<&str> {
        self.session.as_deref()
    }

    pub fn set_session(&mut self, session: Option<String>) {
        self.session = session;
    }

    /// ローカル streaming サーバの再生 URL (`/stream/{urn}`)。
    /// 対応: `desktop/src/lib/streaming.ts` の `buildStreamUrl`。
    pub fn stream_url(&self, urn: &str, hq: bool) -> String {
        let mut url = format!("{}/stream/{}?", self.base, urlencoding::encode(urn));
        if hq {
            url.push_str("hq=true&");
        }
        if let Some(s) = &self.session {
            url.push_str(&format!("session_id={}", urlencoding::encode(s)));
        }
        url
    }

    pub async fn get_json(&self, path: &str) -> Result<Value, String> {
        self.request_json("GET", path, None).await
    }

    pub async fn request_json(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, String> {
        let url = format!("{}{}", self.base, path);
        let mut req = match method {
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            "DELETE" => self.client.delete(&url),
            "PATCH" => self.client.patch(&url),
            _ => self.client.get(&url),
        };
        if let Some(s) = &self.session {
            req = req.header("x-session-id", s);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = tokio::time::timeout(Duration::from_secs(30), req.send())
            .await
            .map_err(|_| format!("timeout: {method} {path}"))?
            .map_err(|e| format!("request: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| format!("body: {e}"))?;
        if !status.is_success() {
            let snippet: String = text.chars().take(200).collect();
            return Err(format!("API {status}: {snippet}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("json: {e}"))
    }
}
