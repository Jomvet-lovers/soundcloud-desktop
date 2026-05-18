use bytes::{Bytes, BytesMut};
use futures::stream::StreamExt;
use reqwest::Client;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::{debug, warn};
use url::Url;

use super::proxy::{fetch_direct_validated, fetch_get_validated, BodyValidator};
use super::validate::{is_valid_audio, is_valid_m3u8};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

const HLS_CONCURRENCY: usize = 3;
const MAX_M3U8_REFRESH: usize = 2;

// (optional fMP4 init, ordered media segments).
pub type SegmentSource = (Option<String>, Vec<String>);

// Re-resolves a fresh playlist when segment tokens expire mid-download.
pub type M3u8Refresher = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<SegmentSource, BoxErr>> + Send>> + Send + Sync,
>;

fn audio_validator() -> BodyValidator {
    Arc::new(|b: &[u8], _: &HashMap<String, String>| is_valid_audio(b))
}

fn m3u8_validator() -> BodyValidator {
    Arc::new(|b: &[u8], _: &HashMap<String, String>| is_valid_m3u8(b))
}

async fn fetch_validated(
    client: &Client,
    proxy_url: &str,
    target_url: &str,
    headers: HashMap<String, String>,
    direct_only: bool,
    validate: BodyValidator,
) -> Result<Bytes, BoxErr> {
    let (data, _) = if direct_only {
        fetch_direct_validated(client, target_url, headers, validate).await?
    } else {
        fetch_get_validated(client, proxy_url, target_url, headers, false, validate).await?
    };
    Ok(data)
}

pub fn parse_m3u8(content: &str, base_url: &str) -> SegmentSource {
    let base = Url::parse(base_url).unwrap_or_else(|_| Url::parse("https://localhost").unwrap());
    let mut init_url = None;
    let mut segment_urls = Vec::new();

    for line in content.lines() {
        let line = line.trim();
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

pub fn mime_to_content_type(mime: &str) -> &'static str {
    match mime {
        "audio/mpeg" | "audio/mpegurl" => "audio/mpeg",
        m if m.contains("mp4a") => "audio/mp4",
        m if m.contains("opus") => "audio/ogg",
        _ => "application/octet-stream",
    }
}

pub async fn fetch_m3u8_source(
    client: &Client,
    proxy_url: &str,
    m3u8_url: &str,
    m3u8_headers: HashMap<String, String>,
    direct_only: bool,
) -> Result<SegmentSource, BoxErr> {
    let data = fetch_validated(
        client,
        proxy_url,
        m3u8_url,
        m3u8_headers,
        direct_only,
        m3u8_validator(),
    )
    .await?;
    let text = String::from_utf8_lossy(&data);
    let source = parse_m3u8(&text, m3u8_url);
    if source.1.is_empty() {
        return Err("no segments found in m3u8".into());
    }
    Ok(source)
}

pub async fn download_progressive(
    client: &Client,
    proxy_url: &str,
    url: &str,
    mime_type: &str,
    extra_headers: HashMap<String, String>,
    direct_only: bool,
) -> Result<(Bytes, &'static str), BoxErr> {
    let data = fetch_validated(
        client,
        proxy_url,
        url,
        extra_headers,
        direct_only,
        audio_validator(),
    )
    .await?;
    Ok((data, mime_to_content_type(mime_type)))
}

// Per-segment proxy↔relay race; on terminal segment failure re-resolve via
// refresher and resume from the failed index without dropping the buffer.
pub async fn download_hls(
    client: &Client,
    proxy_url: &str,
    m3u8_url: &str,
    mime_type: &str,
    m3u8_headers: HashMap<String, String>,
    direct_only: bool,
    refresher: Option<M3u8Refresher>,
) -> Result<(Bytes, &'static str), BoxErr> {
    let (init_url, mut segment_urls) =
        fetch_m3u8_source(client, proxy_url, m3u8_url, m3u8_headers, direct_only).await?;

    let mut buf = BytesMut::new();

    if let Some(ref init) = init_url {
        let data = fetch_validated(
            client,
            proxy_url,
            init,
            HashMap::new(),
            direct_only,
            audio_validator(),
        )
        .await?;
        // Unsupported init payload variant — bail so the caller can fall back.
        if data.windows(4).any(|w| w == b"enca") {
            return Err("unsupported stream".into());
        }
        buf.extend_from_slice(&data);
    }

    let mut results: Vec<Option<Bytes>> = vec![None; segment_urls.len()];
    let mut refreshes_used = 0usize;

    loop {
        let pending: Vec<usize> = results
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_none())
            .map(|(i, _)| i)
            .collect();
        if pending.is_empty() {
            break;
        }

        let failed = fetch_segment_batch(
            client,
            proxy_url,
            &segment_urls,
            &pending,
            direct_only,
            &mut results,
        )
        .await;

        if failed.is_empty() {
            continue;
        }

        let Some(ref refresher) = refresher else {
            return Err(format!("hls: {} segment(s) unrecoverable", failed.len()).into());
        };
        if refreshes_used >= MAX_M3U8_REFRESH {
            return Err("hls: segments still failing after m3u8 refresh".into());
        }

        let (_, fresh_segments) = refresher().await?;
        if fresh_segments.len() != segment_urls.len() {
            return Err("hls: refreshed playlist has a different segment count".into());
        }
        refreshes_used += 1;
        warn!(
            "[hls] re-resolved playlist after {} failed segment(s) (refresh {}/{})",
            failed.len(),
            refreshes_used,
            MAX_M3U8_REFRESH
        );
        segment_urls = fresh_segments;
    }

    for chunk in results.into_iter().flatten() {
        buf.extend_from_slice(&chunk);
    }

    Ok((buf.freeze(), mime_to_content_type(mime_type)))
}

async fn fetch_segment_batch(
    client: &Client,
    proxy_url: &str,
    segment_urls: &[String],
    indices: &[usize],
    direct_only: bool,
    results: &mut [Option<Bytes>],
) -> Vec<usize> {
    let mut stream = futures::stream::iter(indices.iter().copied().map(|idx| {
        let client = client.clone();
        let proxy_url = proxy_url.to_string();
        let url = segment_urls[idx].clone();
        async move {
            let res = fetch_validated(
                &client,
                &proxy_url,
                &url,
                HashMap::new(),
                direct_only,
                audio_validator(),
            )
            .await;
            (idx, res)
        }
    }))
    .buffer_unordered(HLS_CONCURRENCY);

    let mut failed = Vec::new();
    while let Some((idx, res)) = stream.next().await {
        match res {
            Ok(data) => results[idx] = Some(data),
            Err(e) => {
                debug!("[hls] segment {idx} failed: {e}");
                failed.push(idx);
            }
        }
    }
    failed
}
