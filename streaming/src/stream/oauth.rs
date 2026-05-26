use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{info, warn};

use std::sync::Arc;

use super::hls::{download_hls, download_progressive, fetch_m3u8_source, M3u8Refresher};
use super::proxy::fetch_get_json;
use crate::db::postgres::PgPool;

const API_BASE: &str = "https://api.soundcloud.com";
const FALLBACK_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(8);

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

/// Shared per-call infrastructure ctx — HTTP client + DB pool + proxy config
/// passed verbatim through the OAuth fallback chain.
pub struct OauthCtx<'a> {
    pub client: &'a Client,
    pub pg: &'a PgPool,
    pub proxy_url: &'a str,
    pub proxy_fallback: bool,
    pub fallback_session_count: usize,
}

/// Try OAuth API stream: /tracks/{urn}/streams → pick best format → download.
/// `hq_only=true`  → only hls_aac_160 (HQ AAC 160k HLS)
/// `hq_only=false` → all formats: hls_aac_160 → http_mp3_128 → hls_mp3_128
pub async fn try_oauth_stream(
    ctx: &OauthCtx<'_>,
    access_token: &str,
    track_urn: &str,
    secret_token: Option<&str>,
    hq_only: bool,
) -> Option<OAuthStreamResult> {
    let streams = get_streams(ctx, access_token, track_urn, secret_token).await?;

    // hq_only: only HLS AAC 160; otherwise hls_aac_160 first (API v1 path — stable),
    // then progressive mp3, then HLS mp3 fallback
    let candidates: Vec<(&str, &str, &str)> = if hq_only {
        vec![(
            streams.hls_aac_160_url.as_deref(),
            "hls",
            "audio/mp4; codecs=\"mp4a.40.2\"",
        )]
    } else {
        vec![
            (
                streams.hls_aac_160_url.as_deref(),
                "hls",
                "audio/mp4; codecs=\"mp4a.40.2\"",
            ),
            (streams.http_mp3_128_url.as_deref(), "http", "audio/mpeg"),
            (streams.hls_mp3_128_url.as_deref(), "hls", "audio/mpeg"),
        ]
    }
    .into_iter()
    .filter_map(|(url, proto, mime)| url.map(|u| (u, proto, mime)))
    .filter(|(url, _, _)| !url.contains("preview"))
    .collect();

    if candidates.is_empty() {
        return None;
    }

    for (url, proto, mime) in candidates {
        match try_format(
            ctx.client,
            ctx.proxy_url,
            ctx.proxy_fallback,
            access_token,
            url,
            proto,
            mime,
        )
        .await
        {
            Ok(result) => return Some(result),
            Err(e) => {
                warn!("[oauth] format {proto} failed: {e}");
            }
        }
    }

    None
}

enum FetchOutcome {
    Ok(ScStreams),
    Retryable(String), // 401/403/429/5xx/421/network — try next token / fall back
    NotFound,          // 404 — track doesn't exist
}

async fn fetch_streams_direct_once(
    client: &Client,
    target: &str,
    access_token: &str,
) -> FetchOutcome {
    let req = client
        .get(target)
        .header("Authorization", format!("OAuth {access_token}"))
        .header("Accept", "application/json; charset=utf-8")
        .send();

    let resp = match tokio::time::timeout(FALLBACK_ATTEMPT_TIMEOUT, req).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return FetchOutcome::Retryable(format!("send: {e}")),
        Err(_) => return FetchOutcome::Retryable("timeout".into()),
    };

    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        match resp.bytes().await {
            Ok(b) => match serde_json::from_slice::<ScStreams>(&b) {
                Ok(s) => return FetchOutcome::Ok(s),
                Err(e) => return FetchOutcome::Retryable(format!("parse: {e}")),
            },
            Err(e) => return FetchOutcome::Retryable(format!("body: {e}")),
        }
    }

    match status {
        404 => FetchOutcome::NotFound,
        _ => FetchOutcome::Retryable(format!("status {status}")),
    }
}

async fn get_streams(
    ctx: &OauthCtx<'_>,
    access_token: &str,
    track_urn: &str,
    secret_token: Option<&str>,
) -> Option<ScStreams> {
    let mut target = format!("{API_BASE}/tracks/{track_urn}/streams");
    if let Some(st) = secret_token {
        target.push_str(&format!("?secret_token={st}"));
    }

    // 1) direct с original токеном (только когда включён proxy_fallback и
    //    задан proxy). На retryable error падаем на пул oauth_app_tokens.
    if ctx.proxy_fallback && !ctx.proxy_url.is_empty() {
        match fetch_streams_direct_once(ctx.client, &target, access_token).await {
            FetchOutcome::Ok(s) => return Some(s),
            FetchOutcome::NotFound => {
                warn!("[oauth] streams 404 for {track_urn}");
                return None;
            }
            FetchOutcome::Retryable(reason) => {
                warn!(
                    "[oauth] direct streams failed ({reason}) for {track_urn}, trying app-token pool"
                );

                if ctx.fallback_session_count > 0 {
                    match ctx.pg.get_app_tokens(access_token).await {
                        Ok(tokens) if !tokens.is_empty() => {
                            let limit = ctx.fallback_session_count.min(tokens.len());
                            for (i, token) in tokens.iter().take(limit).enumerate() {
                                match fetch_streams_direct_once(ctx.client, &target, token).await {
                                    FetchOutcome::Ok(s) => {
                                        info!(
                                            "[oauth] {track_urn} → app-token direct ({}/{limit})",
                                            i + 1,
                                        );
                                        return Some(s);
                                    }
                                    FetchOutcome::NotFound => {
                                        warn!("[oauth] streams 404 for {track_urn} (app-token)");
                                        return None;
                                    }
                                    FetchOutcome::Retryable(_) => continue,
                                }
                            }
                            warn!(
                                "[oauth] all {limit} app-tokens failed for {track_urn}, falling back to proxy",
                            );
                        }
                        Ok(_) => {
                            warn!("[oauth] app-token pool empty, falling back to proxy");
                        }
                        Err(e) => {
                            warn!("[oauth] failed to fetch app-tokens: {e}, falling back to proxy")
                        }
                    }
                }
            }
        }
    }

    // 3) original logic: proxy with original token (or direct if no proxy configured)
    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), format!("OAuth {access_token}"));
    headers.insert("Accept".into(), "application/json; charset=utf-8".into());

    match fetch_get_json::<ScStreams>(ctx.client, ctx.proxy_url, &target, headers, false).await {
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
    // pf=true: direct → proxy&relay
    if proxy_fallback {
        match try_format_inner(client, proxy_url, access_token, url, proto, mime, true).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                warn!("[oauth] direct format {proto} failed, falling back to proxy&relay: {e}");
            }
        }
    }

    try_format_inner(client, proxy_url, access_token, url, proto, mime, false).await
}

async fn try_format_inner(
    client: &Client,
    proxy_url: &str,
    access_token: &str,
    url: &str,
    proto: &str,
    mime: &str,
    direct_only: bool,
) -> Result<OAuthStreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), format!("OAuth {access_token}"));

    let (data, content_type) = if proto == "hls" {
        // SC regenerates freshly-signed segment URLs every time this redirect
        // URL is fetched with the OAuth header, so the refresher is just the
        // same request again.
        let refresher: M3u8Refresher = {
            let client = client.clone();
            let proxy_url = proxy_url.to_string();
            let url = url.to_string();
            let headers = headers.clone();
            Arc::new(move || {
                let client = client.clone();
                let proxy_url = proxy_url.clone();
                let url = url.clone();
                let headers = headers.clone();
                Box::pin(async move {
                    fetch_m3u8_source(&client, &proxy_url, &url, headers, direct_only).await
                })
            })
        };
        download_hls(
            client,
            proxy_url,
            url,
            mime,
            headers,
            direct_only,
            Some(refresher),
        )
        .await?
    } else {
        download_progressive(client, proxy_url, url, mime, headers, direct_only).await?
    };
    Ok(OAuthStreamResult { data, content_type })
}
