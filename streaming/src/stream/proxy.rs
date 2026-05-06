use base64::Engine;
use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::debug;

const MAX_RETRIES: usize = 3;
const RETRY_DELAYS: [u64; 3] = [300, 800, 2000];

static RELAY: OnceLock<Arc<call_relay::Client>> = OnceLock::new();

pub fn install_relay(relay: Arc<call_relay::Client>) {
    let _ = RELAY.set(relay);
}

type FetchResult = Result<(Bytes, HashMap<String, String>), Box<dyn std::error::Error + Send + Sync>>;

fn proxy_target(
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
) -> (String, HashMap<String, String>) {
    if proxy_url.is_empty() {
        return (target_url.to_string(), extra);
    }
    let mut headers = extra;
    headers.insert(
        "X-Target".into(),
        base64::engine::general_purpose::STANDARD.encode(target_url),
    );
    (proxy_url.to_string(), headers)
}

fn is_retryable_status(status: u16) -> bool {
    status == 421 || status == 429 || (500..=599).contains(&status)
}

async fn http_get_bytes(
    client: &Client,
    url: &str,
    headers: &HashMap<String, String>,
) -> FetchResult {
    let mut last_err: Option<reqwest::Error> = None;
    for attempt in 0..=MAX_RETRIES {
        let mut req = client.get(url);
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..400).contains(&status) {
                    let resp_headers: HashMap<String, String> = resp
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect();
                    match resp.bytes().await {
                        Ok(body) => return Ok((body, resp_headers)),
                        Err(e) => {
                            last_err = Some(e);
                            if attempt < MAX_RETRIES {
                                tokio::time::sleep(Duration::from_millis(
                                    RETRY_DELAYS.get(attempt).copied().unwrap_or(2000),
                                ))
                                .await;
                                continue;
                            }
                            break;
                        }
                    }
                }
                if is_retryable_status(status) && attempt < MAX_RETRIES {
                    debug!("GET {url} → {status}, attempt {attempt}");
                    tokio::time::sleep(Duration::from_millis(
                        RETRY_DELAYS.get(attempt).copied().unwrap_or(2000),
                    ))
                    .await;
                    continue;
                }
                return Err(format!("status {status}").into());
            }
            Err(e) => {
                last_err = Some(e);
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(Duration::from_millis(
                        RETRY_DELAYS.get(attempt).copied().unwrap_or(2000),
                    ))
                    .await;
                }
            }
        }
    }
    Err(last_err
        .map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .unwrap_or_else(|| "fetch failed".into()))
}

async fn via_proxy(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
) -> FetchResult {
    let (url, headers) = proxy_target(proxy_url, target_url, extra);
    http_get_bytes(client, &url, &headers).await
}

async fn via_direct(
    client: &Client,
    target_url: &str,
    extra: HashMap<String, String>,
) -> FetchResult {
    http_get_bytes(client, target_url, &extra).await
}

async fn via_relay(
    relay: Arc<call_relay::Client>,
    target_url: String,
    extra: HashMap<String, String>,
) -> FetchResult {
    let req = call_relay::Request {
        url: target_url,
        method: "GET".to_string(),
        headers: extra,
        body: Bytes::new(),
    };
    match relay.fetch(&req).await {
        Ok(resp) if (200..400).contains(&resp.status) => Ok((resp.body, resp.headers)),
        Ok(resp) => Err(format!("relay status {}", resp.status).into()),
        Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
    }
}

async fn race_relay_proxy(
    client: &Client,
    relay: Arc<call_relay::Client>,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
) -> FetchResult {
    let relay_fut: std::pin::Pin<
        Box<dyn std::future::Future<Output = FetchResult> + Send>,
    > = Box::pin(via_relay(relay, target_url.to_string(), extra.clone()));
    let proxy_fut: std::pin::Pin<
        Box<dyn std::future::Future<Output = FetchResult> + Send>,
    > = Box::pin(via_proxy(client, proxy_url, target_url, extra));
    match futures::future::select_ok(vec![relay_fut, proxy_fut]).await {
        Ok((v, _)) => Ok(v),
        Err(e) => Err(e),
    }
}

/// GET через proxy&relay. Direct никогда не задействуется при `allow_direct=false`.
/// При `allow_direct=true` direct запускается только если proxy_url пуст:
/// сначала direct, при неудаче — relay.
pub async fn fetch_get_bytes(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    allow_direct: bool,
) -> FetchResult {
    let relay = RELAY.get().cloned();
    let proxy_set = !proxy_url.is_empty();

    if proxy_set {
        if let Some(r) = relay {
            return race_relay_proxy(client, r, proxy_url, target_url, extra).await;
        }
        return via_proxy(client, proxy_url, target_url, extra).await;
    }

    // proxy пуст
    if allow_direct {
        match via_direct(client, target_url, extra.clone()).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if let Some(r) = relay {
                    return via_relay(r, target_url.to_string(), extra).await;
                }
                return Err(e);
            }
        }
    }

    if let Some(r) = relay {
        return via_relay(r, target_url.to_string(), extra).await;
    }
    Err("no proxy/relay available and direct disallowed".into())
}

/// GET только напрямую (без proxy и relay).
pub async fn fetch_direct_bytes(
    client: &Client,
    target_url: &str,
    extra: HashMap<String, String>,
) -> FetchResult {
    via_direct(client, target_url, extra).await
}

pub async fn fetch_get_text(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    allow_direct: bool,
) -> Result<(String, HashMap<String, String>), Box<dyn std::error::Error + Send + Sync>> {
    let (bytes, headers) = fetch_get_bytes(client, proxy_url, target_url, extra, allow_direct).await?;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), headers))
}

pub async fn fetch_get_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    allow_direct: bool,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    let (bytes, _) = fetch_get_bytes(client, proxy_url, target_url, extra, allow_direct).await?;
    let val = serde_json::from_slice(&bytes)?;
    Ok(val)
}
