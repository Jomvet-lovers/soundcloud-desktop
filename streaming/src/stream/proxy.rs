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
// Loser of the race still awaited this long (don't drop slow relay early).
const RACE_BOUNDED_GRACE: Duration = Duration::from_secs(15);
const RELAY_MAX_RETRIES: usize = 1;

static RELAY: OnceLock<Arc<call_relay::Client>> = OnceLock::new();

pub fn install_relay(relay: Arc<call_relay::Client>) {
    let _ = RELAY.set(relay);
}

type BoxErr = Box<dyn std::error::Error + Send + Sync>;
type FetchResult = Result<(Bytes, HashMap<String, String>), BoxErr>;

// false => treat like transport failure (retry/keep racing), not the winner.
pub type BodyValidator = Arc<dyn Fn(&[u8], &HashMap<String, String>) -> bool + Send + Sync>;

fn accept_non_empty() -> BodyValidator {
    Arc::new(|b: &[u8], _: &HashMap<String, String>| !b.is_empty())
}

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
    validate: &BodyValidator,
) -> FetchResult {
    let mut last_err: Option<BoxErr> = None;
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
                        Ok(body) => {
                            if validate(&body, &resp_headers) {
                                return Ok((body, resp_headers));
                            }
                            debug!("GET {url} → {status} but body rejected by validator, attempt {attempt}");
                            last_err = Some("invalid response body".into());
                        }
                        Err(e) => last_err = Some(Box::new(e)),
                    }
                } else if is_retryable_status(status) {
                    debug!("GET {url} → {status}, attempt {attempt}");
                    last_err = Some(format!("status {status}").into());
                } else {
                    return Err(format!("status {status}").into());
                }
            }
            Err(e) => last_err = Some(Box::new(e)),
        }
        if attempt < MAX_RETRIES {
            tokio::time::sleep(Duration::from_millis(
                RETRY_DELAYS.get(attempt).copied().unwrap_or(2000),
            ))
            .await;
        }
    }
    Err(last_err.unwrap_or_else(|| "fetch failed".into()))
}

async fn via_proxy(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    validate: &BodyValidator,
) -> FetchResult {
    let (url, headers) = proxy_target(proxy_url, target_url, extra);
    http_get_bytes(client, &url, &headers, validate).await
}

async fn via_direct(
    client: &Client,
    target_url: &str,
    extra: HashMap<String, String>,
    validate: &BodyValidator,
) -> FetchResult {
    http_get_bytes(client, target_url, &extra, validate).await
}

async fn via_relay(
    relay: Arc<call_relay::Client>,
    target_url: String,
    extra: HashMap<String, String>,
    validate: &BodyValidator,
) -> FetchResult {
    let mut last_err: BoxErr = "relay failed".into();
    for attempt in 0..=RELAY_MAX_RETRIES {
        let req = call_relay::Request {
            url: target_url.clone(),
            method: "GET".to_string(),
            headers: extra.clone(),
            body: Bytes::new(),
        };
        match relay.fetch(&req).await {
            Ok(resp) if (200..400).contains(&resp.status) => {
                if validate(&resp.body, &resp.headers) {
                    return Ok((resp.body, resp.headers));
                }
                debug!(
                    "relay {target_url} → {} but body rejected, attempt {attempt}",
                    resp.status
                );
                last_err = "relay invalid response body".into();
            }
            Ok(resp) => {
                last_err = format!("relay status {}", resp.status).into();
            }
            Err(e) => last_err = Box::new(e),
        }
        if attempt < RELAY_MAX_RETRIES {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    Err(last_err)
}

async fn race_relay_proxy(
    client: &Client,
    relay: Arc<call_relay::Client>,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    validate: BodyValidator,
) -> FetchResult {
    let relay_fut = via_relay(relay, target_url.to_string(), extra.clone(), &validate);
    let proxy_fut = via_proxy(client, proxy_url, target_url, extra, &validate);
    tokio::pin!(relay_fut);
    tokio::pin!(proxy_fut);

    tokio::select! {
        relay_res = relay_fut.as_mut() => match relay_res {
            Ok(v) => Ok(v),
            Err(relay_err) => {
                await_other_with_grace(proxy_fut, RACE_BOUNDED_GRACE, relay_err).await
            }
        },
        proxy_res = proxy_fut.as_mut() => match proxy_res {
            Ok(v) => Ok(v),
            Err(proxy_err) => {
                // 502 = proxy front-end down (not a ban): wait relay unbounded.
                let unbounded = proxy_err.to_string() == "status 502";
                if unbounded {
                    match relay_fut.await {
                        Ok(v) => Ok(v),
                        Err(_) => Err(proxy_err),
                    }
                } else {
                    await_other_with_grace(relay_fut, RACE_BOUNDED_GRACE, proxy_err).await
                }
            }
        },
    }
}

async fn await_other_with_grace<F>(other: F, grace: Duration, original_err: BoxErr) -> FetchResult
where
    F: std::future::Future<Output = FetchResult>,
{
    match tokio::time::timeout(grace, other).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) | Err(_) => Err(original_err),
    }
}

pub async fn fetch_get_validated(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    allow_direct: bool,
    validate: BodyValidator,
) -> FetchResult {
    let relay = RELAY.get().cloned();
    let proxy_set = !proxy_url.is_empty();

    if proxy_set {
        if let Some(r) = relay {
            return race_relay_proxy(client, r, proxy_url, target_url, extra, validate).await;
        }
        return via_proxy(client, proxy_url, target_url, extra, &validate).await;
    }

    if allow_direct {
        match via_direct(client, target_url, extra.clone(), &validate).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if let Some(r) = relay {
                    return via_relay(r, target_url.to_string(), extra, &validate).await;
                }
                return Err(e);
            }
        }
    }

    if let Some(r) = relay {
        return via_relay(r, target_url.to_string(), extra, &validate).await;
    }
    Err("no proxy/relay available and direct disallowed".into())
}

async fn via_relay_post(
    relay: Arc<call_relay::Client>,
    target_url: String,
    extra: HashMap<String, String>,
    body: Vec<u8>,
    validate: &BodyValidator,
) -> FetchResult {
    let mut last_err: BoxErr = "relay failed".into();
    for attempt in 0..=RELAY_MAX_RETRIES {
        let req = call_relay::Request {
            url: target_url.clone(),
            method: "POST".to_string(),
            headers: extra.clone(),
            body: Bytes::from(body.clone()),
        };
        match relay.fetch(&req).await {
            Ok(resp) if (200..400).contains(&resp.status) => {
                if validate(&resp.body, &resp.headers) {
                    return Ok((resp.body, resp.headers));
                }
                last_err = "relay invalid response body".into();
            }
            Ok(resp) => last_err = format!("relay status {}", resp.status).into(),
            Err(e) => last_err = Box::new(e),
        }
        if attempt < RELAY_MAX_RETRIES {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    Err(last_err)
}

async fn http_post_bytes(
    client: &Client,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    validate: &BodyValidator,
) -> FetchResult {
    let mut last_err: Option<BoxErr> = None;
    for attempt in 0..=MAX_RETRIES {
        let mut req = client.post(url).body(body.to_vec());
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
                        Ok(b) if validate(&b, &resp_headers) => return Ok((b, resp_headers)),
                        Ok(_) => last_err = Some("invalid response body".into()),
                        Err(e) => last_err = Some(Box::new(e)),
                    }
                } else if is_retryable_status(status) {
                    last_err = Some(format!("status {status}").into());
                } else {
                    return Err(format!("status {status}").into());
                }
            }
            Err(e) => last_err = Some(Box::new(e)),
        }
        if attempt < MAX_RETRIES {
            tokio::time::sleep(Duration::from_millis(
                RETRY_DELAYS.get(attempt).copied().unwrap_or(2000),
            ))
            .await;
        }
    }
    Err(last_err.unwrap_or_else(|| "post failed".into()))
}

pub async fn fetch_post_bytes(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    body: Vec<u8>,
) -> FetchResult {
    let validate = accept_non_empty();
    let relay = RELAY.get().cloned();
    if let Some(r) = relay {
        match via_relay_post(
            r,
            target_url.to_string(),
            extra.clone(),
            body.clone(),
            &validate,
        )
        .await
        {
            Ok(v) => return Ok(v),
            Err(e) => {
                if proxy_url.is_empty() {
                    return Err(e);
                }
            }
        }
    }
    if !proxy_url.is_empty() {
        let (url, headers) = proxy_target(proxy_url, target_url, extra);
        return http_post_bytes(client, &url, &headers, &body, &validate).await;
    }
    Err("no proxy/relay available for POST".into())
}

pub async fn fetch_get_bytes(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    allow_direct: bool,
) -> FetchResult {
    fetch_get_validated(
        client,
        proxy_url,
        target_url,
        extra,
        allow_direct,
        accept_non_empty(),
    )
    .await
}

pub async fn fetch_direct_validated(
    client: &Client,
    target_url: &str,
    extra: HashMap<String, String>,
    validate: BodyValidator,
) -> FetchResult {
    via_direct(client, target_url, extra, &validate).await
}

pub async fn fetch_get_text(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    allow_direct: bool,
) -> Result<(String, HashMap<String, String>), BoxErr> {
    let (bytes, headers) =
        fetch_get_bytes(client, proxy_url, target_url, extra, allow_direct).await?;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), headers))
}

pub async fn fetch_get_json<T: serde::de::DeserializeOwned + 'static>(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
    allow_direct: bool,
) -> Result<T, BoxErr> {
    let validate: BodyValidator =
        Arc::new(|b: &[u8], _: &HashMap<String, String>| serde_json::from_slice::<T>(b).is_ok());
    let (bytes, _) =
        fetch_get_validated(client, proxy_url, target_url, extra, allow_direct, validate).await?;
    let val = serde_json::from_slice(&bytes)?;
    Ok(val)
}
