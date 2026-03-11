use std::path::PathBuf;
use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};

use crate::constants::{is_domain_whitelisted, PROXY_URL};

pub struct State {
    pub assets_dir: PathBuf,
    pub http_client: reqwest::Client,
    pub rt_handle: tokio::runtime::Handle,
}

pub static STATE: OnceLock<State> = OnceLock::new();

pub struct ProxyResult {
    pub status: u16,
    pub content_type: String,
    pub data: Vec<u8>,
}

fn cache_key(url: &str) -> String {
    hex::encode(Sha256::digest(url.as_bytes()))
}

fn extension_from_content_type(ct: &str) -> &str {
    if ct.starts_with("image/jpeg") {
        ".jpg"
    } else if ct.starts_with("image/png") {
        ".png"
    } else if ct.starts_with("image/webp") {
        ".webp"
    } else if ct.starts_with("image/gif") {
        ".gif"
    } else if ct.starts_with("image/svg") {
        ".svg"
    } else if ct.contains("font") {
        ".font"
    } else if ct.starts_with("text/css") {
        ".css"
    } else if ct.contains("javascript") {
        ".js"
    } else {
        ".bin"
    }
}

fn content_type_from_ext(ext: &str) -> &str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "css" => "text/css",
        "js" => "application/javascript",
        _ => "application/octet-stream",
    }
}

fn find_cached_file(dir: &PathBuf, key: &str) -> Option<PathBuf> {
    let read_dir = std::fs::read_dir(dir).ok()?;
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_str()?;
        if name_str.starts_with(key) {
            return Some(entry.path());
        }
    }
    None
}

/// Core proxy logic — shared between scproxy:// protocol and HTTP proxy server.
/// `encoded` is a (possibly percent-encoded) base64 target URL.
pub async fn proxy_request(encoded: &str) -> ProxyResult {
    let state = match STATE.get() {
        Some(s) => s,
        None => {
            return ProxyResult {
                status: 503,
                content_type: "text/plain".into(),
                data: b"not ready".to_vec(),
            }
        }
    };

    let decoded = urlencoding::decode(encoded).unwrap_or_default();
    let target_url = match BASE64.decode(decoded.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                return ProxyResult {
                    status: 400,
                    content_type: "text/plain".into(),
                    data: b"invalid utf8".to_vec(),
                }
            }
        },
        Err(_) => {
            return ProxyResult {
                status: 400,
                content_type: "text/plain".into(),
                data: b"invalid base64".to_vec(),
            }
        }
    };

    let host = target_url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|authority| authority.split(':').next())
        .unwrap_or("");

    if is_domain_whitelisted(host) {
        return ProxyResult {
            status: 403,
            content_type: "text/plain".into(),
            data: b"whitelisted domain".to_vec(),
        };
    }

    // Cache check
    let key = cache_key(&target_url);
    if let Some(cached) = find_cached_file(&state.assets_dir, &key) {
        let ext = cached
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin");
        let ct = content_type_from_ext(ext);
        if let Ok(data) = tokio::fs::read(&cached).await {
            #[cfg(debug_assertions)]
            println!("[Proxy] cache HIT {}", target_url);
            return ProxyResult {
                status: 200,
                content_type: ct.to_string(),
                data,
            };
        }
    }

    #[cfg(debug_assertions)]
    println!("[Proxy] {} -> upstream", target_url);

    // Upstream fetch
    let encoded_for_header = BASE64.encode(target_url.as_bytes());
    let upstream = match state
        .http_client
        .get(PROXY_URL)
        .header("X-Target", &encoded_for_header)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return ProxyResult {
                status: 502,
                content_type: "text/plain".into(),
                data: format!("upstream error: {e}").into_bytes(),
            }
        }
    };

    let status = upstream.status().as_u16();
    let content_type = upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let data = match upstream.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            return ProxyResult {
                status: 502,
                content_type: "text/plain".into(),
                data: format!("body read error: {e}").into_bytes(),
            }
        }
    };

    // Cache in background
    if status == 200 {
        let ext = extension_from_content_type(&content_type);
        let cache_path = state.assets_dir.join(format!("{key}{ext}"));
        let data_clone = data.clone();
        tokio::spawn(async move {
            let _ = tokio::fs::write(&cache_path, &data_clone).await;
        });
    }

    ProxyResult {
        status,
        content_type,
        data,
    }
}

/// Handler for scproxy:// URI scheme protocol (used by img.src hooks)
pub async fn handle_uri(request: http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
    let encoded = request.uri().path().trim_start_matches('/');
    let result = proxy_request(encoded).await;
    http::Response::builder()
        .status(result.status)
        .header("content-type", &result.content_type)
        .header("access-control-allow-origin", "*")
        .body(result.data)
        .unwrap()
}
