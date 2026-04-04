use bytes::{Bytes, BytesMut};
use futures::stream::{self, Stream};
use reqwest::Client;
use std::collections::HashMap;
use std::pin::Pin;
use tracing::warn;
use url::Url;

use super::proxy::proxy_get_bytes;

const HLS_PREFETCH_SEGMENTS: usize = 3;

/// Parse m3u8 playlist → (init_url, segment_urls)
pub fn parse_m3u8(content: &str, base_url: &str) -> (Option<String>, Vec<String>) {
    let base = Url::parse(base_url).unwrap_or_else(|_| Url::parse("https://localhost").unwrap());
    let mut init_url = None;
    let mut segment_urls = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        // #EXT-X-MAP:URI="..."
        if let Some(start) = line.find("#EXT-X-MAP:URI=\"") {
            let rest = &line[start + 16..];
            if let Some(end) = rest.find('"') {
                init_url = Some(resolve_url(&rest[..end], &base));
            }
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        segment_urls.push(resolve_url(line, &base));
    }

    (init_url, segment_urls)
}

fn resolve_url(url: &str, base: &Url) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    base.join(url)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| url.to_string())
}

/// Map SC mime type to content-type header
pub fn mime_to_content_type(mime: &str) -> &'static str {
    match mime {
        "audio/mpeg" | "audio/mpegurl" => "audio/mpeg",
        m if m.contains("mp4a") => "audio/mp4",
        m if m.contains("opus") => "audio/ogg",
        _ => "application/octet-stream",
    }
}

/// Download all HLS segments into a single Bytes buffer (for tee to CDN).
/// Returns (audio_bytes, content_type).
pub async fn download_hls_full(
    client: &Client,
    proxy_url: &str,
    m3u8_url: &str,
    mime_type: &str,
) -> Result<(Bytes, &'static str), Box<dyn std::error::Error + Send + Sync>> {
    let (m3u8_text, _) = proxy_get_bytes(client, proxy_url, m3u8_url, HashMap::new()).await?;
    let m3u8_content = String::from_utf8_lossy(&m3u8_text);
    let (init_url, segment_urls) = parse_m3u8(&m3u8_content, m3u8_url);

    if segment_urls.is_empty() {
        return Err("No segments found in m3u8".into());
    }

    let mut buf = BytesMut::new();

    // Download init segment
    if let Some(ref init) = init_url {
        let (data, _) = proxy_get_bytes(client, proxy_url, init, HashMap::new()).await?;
        // Check CENC encryption
        if data.windows(4).any(|w| w == b"enca") {
            return Err("Stream is CENC encrypted".into());
        }
        buf.extend_from_slice(&data);
    }

    // Download segments with prefetch queue
    let mut inflight: Vec<tokio::task::JoinHandle<Result<Bytes, reqwest::Error>>> = Vec::new();
    let mut next_idx = 0;

    let fill_queue =
        |inflight: &mut Vec<tokio::task::JoinHandle<Result<Bytes, reqwest::Error>>>,
         next_idx: &mut usize,
         client: &Client,
         proxy_url: &str,
         urls: &[String]| {
            while *next_idx < urls.len() && inflight.len() < HLS_PREFETCH_SEGMENTS {
                let c = client.clone();
                let p = proxy_url.to_string();
                let u = urls[*next_idx].clone();
                inflight.push(tokio::spawn(async move {
                    let (data, _) = proxy_get_bytes(&c, &p, &u, HashMap::new()).await?;
                    Ok(data)
                }));
                *next_idx += 1;
            }
        };

    fill_queue(
        &mut inflight,
        &mut next_idx,
        client,
        proxy_url,
        &segment_urls,
    );

    while !inflight.is_empty() {
        let handle = inflight.remove(0);
        match handle.await {
            Ok(Ok(chunk)) => buf.extend_from_slice(&chunk),
            Ok(Err(e)) => {
                warn!("HLS segment download error: {e}");
                return Err(e.into());
            }
            Err(e) => {
                warn!("HLS segment task panic: {e}");
                return Err(e.into());
            }
        }
        fill_queue(
            &mut inflight,
            &mut next_idx,
            client,
            proxy_url,
            &segment_urls,
        );
    }

    let content_type = mime_to_content_type(mime_type);
    Ok((buf.freeze(), content_type))
}

/// Stream HLS segments as a bytes stream (for direct client response).
/// Returns (stream, content_type).
pub async fn stream_hls(
    client: &Client,
    proxy_url: &str,
    m3u8_url: &str,
    mime_type: &str,
) -> Result<
    (
        Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        &'static str,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let (data, content_type) = download_hls_full(client, proxy_url, m3u8_url, mime_type).await?;

    let stream = stream::once(async move { Ok(data) });
    Ok((Box::pin(stream), content_type))
}
