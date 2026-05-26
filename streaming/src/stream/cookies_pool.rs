//! Пул `CookiesClient`'ов с ротацией по 429. Каждая строка cookies из
//! `SC_COOKIES` — отдельная сессия; начинаем с последней успешной, на
//! rate-limit переходим к следующей. Кончились все — отдаём последнюю ошибку.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::anon::AnonClient;
use super::cookies::{CookieStreamResult, CookiesClient};
use super::restricted::{RestrictedSource, Transcoding};
use crate::config::parse_cookie_value;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(30);

struct PoolEntry {
    client: CookiesClient,
    /// Когда можно снова пробовать (после 429).
    rate_limited_until: Mutex<Option<tokio::time::Instant>>,
}

pub struct CookiesPool {
    entries: Vec<PoolEntry>,
    cursor: AtomicUsize,
}

impl CookiesPool {
    /// Строит пул. Cookies-строки без `oauth_token=` тихо отбрасываются.
    pub fn new(http: Client, proxy_url: &str, cookies_list: &[String]) -> Self {
        let entries = cookies_list
            .iter()
            .filter_map(|raw| {
                let token = parse_cookie_value(raw, "oauth_token")?;
                let client = CookiesClient::new(
                    http.clone(),
                    proxy_url.to_string(),
                    raw.clone(),
                    token,
                    AnonClient::new(http.clone(), proxy_url.to_string()),
                );
                Some(PoolEntry {
                    client,
                    rate_limited_until: Mutex::new(None),
                })
            })
            .collect::<Vec<_>>();
        Self {
            entries,
            cursor: AtomicUsize::new(0),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    async fn try_rotate<'a, F, Fut, T>(&'a self, mut op: F) -> Result<T, BoxErr>
    where
        F: FnMut(&'a CookiesClient) -> Fut,
        Fut: std::future::Future<Output = Result<T, BoxErr>>,
    {
        let n = self.entries.len();
        if n == 0 {
            return Err("cookies pool empty".into());
        }
        let start = self.cursor.load(Ordering::Relaxed) % n;
        let mut last_err: Option<BoxErr> = None;
        let now = tokio::time::Instant::now();

        for off in 0..n {
            let idx = (start + off) % n;
            let entry = &self.entries[idx];

            if let Some(until) = *entry.rate_limited_until.lock().await {
                if until > now {
                    continue;
                }
            }

            match op(&entry.client).await {
                Ok(v) => {
                    self.cursor.store(idx, Ordering::Relaxed);
                    *entry.rate_limited_until.lock().await = None;
                    return Ok(v);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if is_rate_limited(&msg) {
                        warn!("[cookies-pool] client #{idx} rate-limited: {msg}");
                        *entry.rate_limited_until.lock().await = Some(now + RATE_LIMIT_COOLDOWN);
                        last_err = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| "all cookies clients rate-limited".into()))
    }

    pub async fn get_stream(
        self: &Arc<Self>,
        track_urn: &str,
        hq_only: bool,
    ) -> Result<Option<CookieStreamResult>, BoxErr> {
        self.try_rotate(|client| async move { client.get_stream(track_urn, hq_only).await })
            .await
    }

    pub async fn fetch_track_meta(
        self: &Arc<Self>,
        track_urn: &str,
    ) -> Result<
        (
            Vec<Transcoding>,
            Option<String>,
            String,
            HashMap<String, String>,
        ),
        BoxErr,
    > {
        self.try_rotate(|client| async move {
            let (tcs, auth, cid) = client.fetch_track_meta(track_urn).await?;
            Ok((tcs, auth, cid, client.cookie_auth_headers()))
        })
        .await
    }

    pub(crate) async fn resolve_restricted(
        self: &Arc<Self>,
        track_urn: &str,
        hq_first: bool,
    ) -> Result<Option<RestrictedSource>, BoxErr> {
        self.try_rotate(
            |client| async move { client.resolve_restricted(track_urn, hq_first).await },
        )
        .await
    }

    pub fn log_summary(&self) {
        info!(
            "[cookies-pool] initialized with {} client(s)",
            self.entries.len()
        );
    }
}

fn is_rate_limited(msg: &str) -> bool {
    msg.contains("429") || msg.to_ascii_lowercase().contains("too many requests")
}

#[cfg(test)]
mod tests {
    use super::is_rate_limited;

    #[test]
    fn detects_429_variants() {
        assert!(is_rate_limited("status 429"));
        assert!(is_rate_limited("HTTP 429 Too Many Requests"));
        assert!(is_rate_limited("relay status 429"));
        assert!(!is_rate_limited("status 404"));
        assert!(!is_rate_limited("status 502"));
        assert!(!is_rate_limited("connection reset"));
    }
}
