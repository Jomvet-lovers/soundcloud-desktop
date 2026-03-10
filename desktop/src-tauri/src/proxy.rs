use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::TryStreamExt;
use sha2::{Digest, Sha256};
use warp::http::{Response, StatusCode};
use warp::hyper::Body;

use crate::constants::{is_domain_whitelisted, PROXY_URL};

const CACHEABLE_CONTENT_TYPES: &[&str] = &[
    "image/",
    "font/",
    "application/font",
    "text/css",
    "application/javascript",
    "application/wasm",
];

fn cache_key(url: &str) -> String {
    let hash = Sha256::digest(url.as_bytes());
    hex::encode(hash)
}

fn is_cacheable_content_type(ct: &str) -> bool {
    let ct_lower = ct.to_ascii_lowercase();
    CACHEABLE_CONTENT_TYPES.iter().any(|p| ct_lower.starts_with(p))
}

fn is_cache_allowed(cache_control: Option<&str>) -> bool {
    match cache_control {
        None => true,
        Some(cc) => {
            let cc_lower = cc.to_ascii_lowercase();
            !cc_lower.contains("no-store") && !cc_lower.contains("no-cache")
        }
    }
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

pub async fn handle_proxy(
    encoded_url: String,
    method: warp::http::Method,
    headers: warp::http::HeaderMap,
    body: warp::hyper::body::Bytes,
    http_client: reqwest::Client,
    assets_dir: PathBuf,
) -> Result<Response<Body>, warp::Rejection> {
    let target_url = match BASE64.decode(encoded_url.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from("invalid utf8"))
                    .unwrap());
            }
        },
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("invalid base64"))
                .unwrap());
        }
    };

    let host = target_url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|authority| authority.split(':').next())
        .unwrap_or("");

    if is_domain_whitelisted(host) {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("whitelisted domain"))
            .unwrap());
    }

    // Check cache for GET requests
    let is_get = method == warp::http::Method::GET;
    if is_get {
        let key = cache_key(&target_url);
        // Find cached file (key.ext pattern)
        if let Some(cached) = find_cached_file(&assets_dir, &key) {
            let ext = cached
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin");
            let ct = content_type_from_ext(ext);
            match tokio::fs::read(&cached).await {
                Ok(data) => {
                    #[cfg(debug_assertions)]
                    println!("[Proxy] cache HIT {}", target_url);
                    return Ok(Response::builder()
                        .status(StatusCode::OK)
                        .header("Content-Type", ct)
                        .header("Content-Length", data.len().to_string())
                        .header("X-Cache", "HIT")
                        .body(Body::from(data))
                        .unwrap());
                }
                Err(_) => {
                    // Cache file unreadable, fall through to upstream
                }
            }
        }
    }

    let encoded_for_header = BASE64.encode(target_url.as_bytes());
    #[cfg(debug_assertions)]
    println!("[Proxy] {} {} -> upstream", method, target_url);

    let reqwest_method =
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET);

    let mut req = http_client
        .request(reqwest_method, PROXY_URL)
        .header("X-Target", &encoded_for_header);

    for (key, value) in headers.iter() {
        let name = key.as_str();
        if matches!(
            name,
            "content-type" | "range" | "accept" | "accept-encoding" | "authorization"
        ) {
            req = req.header(name, value.as_bytes());
        }
    }

    if !body.is_empty() {
        req = req.body(body.to_vec());
    }

    let upstream = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("[Proxy] upstream error: {e}");
            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from(format!("upstream error: {e}")))
                .unwrap());
        }
    };

    let status = upstream.status().as_u16();

    let content_type = upstream
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let cache_control = upstream
        .headers()
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let should_cache = is_get
        && status == 200
        && is_cacheable_content_type(&content_type)
        && is_cache_allowed(cache_control.as_deref());

    let mut builder = Response::builder().status(status);

    for (key, value) in upstream.headers().iter() {
        let name = key.as_str();
        if matches!(
            name,
            "content-type"
                | "content-length"
                | "cache-control"
                | "etag"
                | "last-modified"
                | "accept-ranges"
                | "content-range"
        ) {
            builder = builder.header(name, value.as_bytes());
        }
    }

    if should_cache {
        // Read full body, cache to disk, return
        let data = match upstream.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from(format!("body read error: {e}")))
                    .unwrap());
            }
        };

        let key = cache_key(&target_url);
        let ext = extension_from_content_type(&content_type);
        let cache_path = assets_dir.join(format!("{key}{ext}"));
        // Write cache in background — don't block response
        let data_clone = data.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::fs::write(&cache_path, &data_clone).await {
                #[cfg(debug_assertions)]
                eprintln!("[Proxy] cache write error: {e}");
            }
        });

        builder = builder.header("X-Cache", "MISS");
        Ok(builder.body(Body::from(data)).unwrap())
    } else {
        // Stream non-cacheable responses
        let stream = upstream
            .bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
        Ok(builder.body(Body::wrap_stream(stream)).unwrap())
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

fn content_type_from_ext(ext: &str) -> &str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "css" => "text/css",
        "js" => "application/javascript",
        "font" => "application/octet-stream",
        _ => "application/octet-stream",
    }
}