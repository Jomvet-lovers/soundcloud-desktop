use base64::Engine;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tracing::warn;

const MAX_RETRIES: usize = 3;
const RETRY_DELAYS: [u64; 3] = [300, 800, 2000];

/// Build proxied request URL + headers.
/// If proxy_url is set, rewrites to proxy with X-Target: base64(target_url).
pub fn proxy_target(
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

/// GET with retry through proxy. Returns (body_bytes, response_headers).
pub async fn proxy_get_bytes(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
) -> Result<(bytes::Bytes, HashMap<String, String>), reqwest::Error> {
    let (url, headers) = proxy_target(proxy_url, target_url, extra);

    let mut last_err = None;

    for attempt in 0..=MAX_RETRIES {
        let mut req = client.get(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v.as_str());
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status >= 200 && status < 300 {
                    let resp_headers: HashMap<String, String> = resp
                        .headers()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect();
                    let body = resp.bytes().await?;
                    return Ok((body, resp_headers));
                }
                if is_retryable_status(status) {
                    warn!("proxy GET {target_url} → {status}, attempt {attempt}");
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(Duration::from_millis(
                            RETRY_DELAYS.get(attempt).copied().unwrap_or(2000),
                        ))
                        .await;
                        continue;
                    }
                }
                // Non-retryable or exhausted retries — return error
                let err_resp = resp.error_for_status();
                match err_resp {
                    Err(e) => return Err(e),
                    Ok(_) => unreachable!(),
                }
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

    Err(last_err.unwrap())
}

/// GET text with retry through proxy.
pub async fn proxy_get_text(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
) -> Result<(String, HashMap<String, String>), reqwest::Error> {
    let (bytes, headers) = proxy_get_bytes(client, proxy_url, target_url, extra).await?;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), headers))
}

/// GET JSON with retry through proxy.
pub async fn proxy_get_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    extra: HashMap<String, String>,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    let (bytes, _) = proxy_get_bytes(client, proxy_url, target_url, extra).await?;
    let val = serde_json::from_slice(&bytes)?;
    Ok(val)
}
