use std::path::PathBuf;
use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};
use tokio::fs;

use crate::network::upload_grants::{self, GrantError};
use crate::shared::constants::is_domain_whitelisted;

pub struct State {
    pub assets_dir: PathBuf,
    pub http_client: wreq::Client,
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

/// Sniff the content type from the leading bytes. Single source of truth so we
/// don't need a sidecar .meta file on disk.
fn sniff_content_type(data: &[u8]) -> &'static str {
    if data.len() >= 3 && data[..3] == [0xFF, 0xD8, 0xFF] {
        "image/jpeg"
    } else if data.len() >= 8 && data[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        "image/png"
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        "image/webp"
    } else if data.len() >= 6 && (&data[..6] == b"GIF87a" || &data[..6] == b"GIF89a") {
        "image/gif"
    } else if data.len() >= 12
        && &data[4..8] == b"ftyp"
        && (&data[8..12] == b"avif" || &data[8..12] == b"avis")
    {
        "image/avif"
    } else if data.len() >= 4 && data[..4] == [0x00, 0x00, 0x01, 0x00] {
        "image/x-icon"
    } else if data.len() >= 5 && (&data[..5] == b"<?xml" || &data[..4] == b"<svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

async fn write_cached_asset(cache_path: PathBuf, data: Vec<u8>) {
    let tmp = PathBuf::from(format!(
        "{}.tmp-{}",
        cache_path.display(),
        std::process::id()
    ));
    if fs::write(&tmp, &data).await.is_err() {
        let _ = fs::remove_file(&tmp).await;
        return;
    }
    if fs::rename(&tmp, &cache_path).await.is_err() {
        let _ = fs::remove_file(&tmp).await;
    }
}

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
    let payload_bytes = match BASE64.decode(decoded.as_bytes()) {
        Ok(bytes) => bytes,
        Err(_) => {
            return ProxyResult {
                status: 400,
                content_type: "text/plain".into(),
                data: b"invalid base64".to_vec(),
            }
        }
    };

    let payload: Vec<String> = match serde_json::from_slice(&payload_bytes) {
        Ok(v) => v,
        Err(_) => {
            return ProxyResult {
                status: 400,
                content_type: "text/plain".into(),
                data: b"invalid payload".to_vec(),
            }
        }
    };

    let target_url = match payload.first() {
        Some(s) if !s.is_empty() => s.clone(),
        _ => {
            return ProxyResult {
                status: 400,
                content_type: "text/plain".into(),
                data: b"missing target".to_vec(),
            }
        }
    };
    let upstreams = &payload[1..];
    if upstreams.is_empty() {
        return ProxyResult {
            status: 400,
            content_type: "text/plain".into(),
            data: b"missing upstream".to_vec(),
        };
    }

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

    // The asset proxy is deliberately open to third-party image hosts
    // (wallpapers), so this cannot be an allowlist: the ceiling is "public
    // http(s) only". Without it, any script in a webview could read the local
    // API, the intranet or the cloud metadata endpoint through `scproxy://`.
    if is_blocked_target(&target_url) {
        return ProxyResult {
            status: 403,
            content_type: "text/plain".into(),
            data: b"blocked target".to_vec(),
        };
    }

    let cache_path = state.assets_dir.join(cache_key(&target_url));

    if let Ok(data) = fs::read(&cache_path).await {
        if !data.is_empty() {
            #[cfg(debug_assertions)]
            println!("[Proxy] cache HIT {}", target_url);
            let content_type = sniff_content_type(&data).to_string();
            return ProxyResult {
                status: 200,
                content_type,
                data,
            };
        }
        let _ = fs::remove_file(&cache_path).await;
    }

    #[cfg(debug_assertions)]
    println!("[Proxy] {} -> upstream", target_url);

    let encoded_for_header = BASE64.encode(target_url.as_bytes());
    let mut status = 502u16;
    let mut data: Vec<u8> = Vec::new();

    for hop in crate::network::edge::expand_upstreams(upstreams) {
        // `direct` = fetch the target ourselves with a browser User-Agent
        // (hosts like wallhaven/konachan 403 a non-browser UA). Otherwise relay
        // the request to the proxy upstream via the X-Target header.
        let builder = if hop.url == "direct" {
            state
                .http_client
                .get(target_url.as_str())
                .header("User-Agent", crate::network::BROWSER_UA)
        } else {
            state
                .http_client
                .get(&hop.url)
                .header("X-Target", &encoded_for_header)
        };
        let resp = match builder.send().await {
            Ok(r) => r,
            Err(_) => {
                hop.note(false);
                continue;
            }
        };

        // A redirect can land on a private host even when the requested URL was
        // public; refuse the body in that case instead of handing it back.
        if resp.url().host_str().map(is_blocked_host).unwrap_or(true) {
            hop.note(false);
            continue;
        }

        status = resp.status().as_u16();
        if !crate::network::edge::hop_ok(&hop, &resp) {
            continue;
        }
        match resp.bytes().await {
            Ok(b) => data = b.to_vec(),
            Err(_) => {
                hop.note(false);
                continue;
            }
        }

        hop.note(status < 500);
        if status < 500 {
            break;
        }
    }

    let content_type = if status == 200 && !data.is_empty() {
        sniff_content_type(&data).to_string()
    } else {
        String::new()
    };

    let is_cacheable = status == 200 && !data.is_empty() && content_type.starts_with("image/");
    if is_cacheable {
        let data_clone = data.clone();
        let path = cache_path.clone();
        tokio::spawn(async move {
            write_cached_asset(path, data_clone).await;
        });
    }

    ProxyResult {
        status,
        content_type,
        data,
    }
}

/// Long browser cache for successful image responses. Image URLs are
/// content-addressable (sndcdn artwork-XXX), so effectively immutable. Without
/// this the WebView re-hits the proxy on every render and the disk cache only
/// saves a network roundtrip — not the disk read.
pub fn cache_control_for(status: u16) -> &'static str {
    if status == 200 {
        "public, max-age=31536000, immutable"
    } else {
        "no-store"
    }
}

fn upload_response(status: u16) -> http::response::Builder {
    let mut builder = http::Response::builder().status(status);
    for (name, value) in upload_grants::cors_headers() {
        builder = builder.header(name, value);
    }
    builder
}

fn upload_preflight_response() -> http::Response<Vec<u8>> {
    let mut builder = http::Response::builder().status(204);
    for (name, value) in upload_grants::preflight_cors_headers() {
        builder = builder.header(name, value);
    }
    builder.body(Vec::new()).unwrap()
}

fn grant_error_response(error: GrantError) -> http::Response<Vec<u8>> {
    upload_response(error.status())
        .header("content-type", "text/plain")
        .header("cache-control", "no-store")
        .body(error.message().as_bytes().to_vec())
        .unwrap()
}

pub async fn handle_uri(request: http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
    let path = request.uri().path();

    if let Some(token) = path.strip_prefix("/up/") {
        if request.method() == http::Method::OPTIONS {
            return upload_preflight_response();
        }
        return match upload_grants::read_grant(token).await {
            Ok((data, grant)) => upload_response(200)
                .header("content-type", &grant.content_type)
                .header("content-length", data.len())
                .header("cache-control", "no-store")
                .body(data)
                .unwrap(),
            Err(error) => grant_error_response(error),
        };
    }

    if let Some(encoded) = path.strip_prefix("/img/") {
        let result = crate::network::image_cache::handle(encoded).await;
        return http::Response::builder()
            .status(result.status)
            .header("content-type", &result.content_type)
            .header("cache-control", cache_control_for(result.status))
            .header("access-control-allow-origin", "*")
            .body(result.data)
            .unwrap();
    }

    let encoded = path.trim_start_matches('/');
    let result = proxy_request(encoded).await;
    http::Response::builder()
        .status(result.status)
        .header("content-type", &result.content_type)
        .header("cache-control", cache_control_for(result.status))
        .header("access-control-allow-origin", "*")
        .body(result.data)
        .unwrap()
}

/// Whether a proxy target points at this machine or the private network.
///
/// The proxy is reachable from every webview in the app, so an unrestricted
/// target is a read primitive for the local direct API, intranet services and
/// the cloud metadata endpoint. Only `http(s)` to a public host passes.
fn is_blocked_target(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return true;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return true;
    }
    match parsed.host_str() {
        Some(host) => is_blocked_host(host),
        None => true,
    }
}

fn is_blocked_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host.is_empty() {
        return true;
    }
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host == "metadata.google.internal"
    {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => is_blocked_ip(ip),
        Err(_) => false,
    }
}

fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_unspecified()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (seg & 0xffc0) == 0xfe80
                || (seg & 0xfe00) == 0xfc00
                || v6.to_ipv4_mapped()
                    .map(|v4| is_blocked_ip(IpAddr::V4(v4)))
                    .unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_blocked_host, is_blocked_target};

    #[test]
    fn local_and_private_targets_are_blocked() {
        for url in [
            "http://127.0.0.1:8090/me",
            "http://localhost:1420/",
            "http://[::1]/",
            "http://scproxy-1.localhost/p/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://192.168.1.1/",
            "http://10.0.0.5/",
            "http://172.16.0.1/",
            "http://0.0.0.0/",
            "file:///etc/passwd",
            "ftp://example.com/x",
            "not-a-url",
        ] {
            assert!(is_blocked_target(url), "{url} should be blocked");
        }
    }

    #[test]
    fn public_hosts_still_pass() {
        for url in [
            "https://i1.sndcdn.com/artworks-000-large.jpg",
            "https://cf-media.sndcdn.com/abc.mp3",
            "https://images.unsplash.com/photo-1.jpg",
            "http://example.com/a.png",
        ] {
            assert!(!is_blocked_target(url), "{url} should pass");
        }
        assert!(!is_blocked_host("example.com"));
    }
}
