use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use tracing::warn;

use super::hls::download_hls_full;
use super::proxy::{proxy_get_bytes, proxy_get_json};

const API_BASE: &str = "https://api.soundcloud.com";

#[derive(Debug, serde::Deserialize)]
pub struct ScStreams {
    pub hls_aac_160_url: Option<String>,
    pub http_mp3_128_url: Option<String>,
    pub hls_mp3_128_url: Option<String>,
}

/// Stream result: full audio data + content_type + quality tag
pub struct OAuthStreamResult {
    pub data: Bytes,
    pub content_type: &'static str,
}

/// Try OAuth API stream: /tracks/{urn}/streams → pick best format → download.
pub async fn try_oauth_stream(
    client: &Client,
    proxy_url: &str,
    proxy_fallback: bool,
    access_token: &str,
    track_urn: &str,
    secret_token: Option<&str>,
) -> Option<OAuthStreamResult> {
    let streams = get_streams(client, proxy_url, proxy_fallback, access_token, track_urn, secret_token).await?;

    // Format priority: HLS AAC 160 → HTTP MP3 128 → HLS MP3 128
    let candidates: Vec<(&str, &str, &str)> = [
        (
            streams.hls_aac_160_url.as_deref(),
            "hls",
            "audio/mp4; codecs=\"mp4a.40.2\"",
        ),
        (streams.http_mp3_128_url.as_deref(), "http", "audio/mpeg"),
        (streams.hls_mp3_128_url.as_deref(), "hls", "audio/mpeg"),
    ]
    .into_iter()
    .filter_map(|(url, proto, mime)| url.map(|u| (u, proto, mime)))
    .collect();

    if candidates.is_empty() {
        return None;
    }

    for (url, proto, mime) in candidates {
        match try_format(client, proxy_url, proxy_fallback, access_token, url, proto, mime).await {
            Ok(result) => return Some(result),
            Err(e) => {
                warn!("[oauth] format {proto} failed: {e}");
            }
        }
    }

    None
}

async fn get_streams(
    client: &Client,
    proxy_url: &str,
    proxy_fallback: bool,
    access_token: &str,
    track_urn: &str,
    secret_token: Option<&str>,
) -> Option<ScStreams> {
    let mut target = format!("{API_BASE}/tracks/{track_urn}/streams");
    if let Some(st) = secret_token {
        target.push_str(&format!("?secret_token={st}"));
    }

    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), format!("OAuth {access_token}"));
    headers.insert("Accept".into(), "application/json; charset=utf-8".into());

    // If proxy_fallback: try direct first, then via proxy on error
    if proxy_fallback && !proxy_url.is_empty() {
        match proxy_get_json::<ScStreams>(client, "", &target, headers.clone()).await {
            Ok(s) => return Some(s),
            Err(e) => {
                warn!("[oauth] direct get streams failed, falling back to proxy: {e}");
            }
        }
    }

    match proxy_get_json::<ScStreams>(client, proxy_url, &target, headers).await {
        Ok(s) => Some(s),
        Err(e) => {
            warn!("[oauth] get streams failed: {e}");
            None
        }
    }
}

async fn try_format(
    client: &Client,
    proxy_url: &str,
    proxy_fallback: bool,
    access_token: &str,
    url: &str,
    proto: &str,
    mime: &str,
) -> Result<OAuthStreamResult, Box<dyn std::error::Error + Send + Sync>> {
    // If proxy_fallback: try direct first, then via proxy
    if proxy_fallback && !proxy_url.is_empty() {
        match try_format_inner(client, "", access_token, url, proto, mime).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                warn!("[oauth] direct format {proto} failed, falling back to proxy: {e}");
            }
        }
    }

    try_format_inner(client, proxy_url, access_token, url, proto, mime).await
}

async fn try_format_inner(
    client: &Client,
    proxy_url: &str,
    access_token: &str,
    url: &str,
    proto: &str,
    mime: &str,
) -> Result<OAuthStreamResult, Box<dyn std::error::Error + Send + Sync>> {
    if proto == "hls" {
        let (data, content_type) = download_hls_full(client, proxy_url, url, mime).await?;
        Ok(OAuthStreamResult { data, content_type })
    } else {
        // HTTP direct download
        let mut headers = HashMap::new();
        headers.insert("Authorization".into(), format!("OAuth {access_token}"));
        let (data, _) = proxy_get_bytes(client, proxy_url, url, headers).await?;
        Ok(OAuthStreamResult {
            data,
            content_type: "audio/mpeg",
        })
    }
}
