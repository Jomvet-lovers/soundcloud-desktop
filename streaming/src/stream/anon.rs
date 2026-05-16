use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::hls::{download_hls, download_progressive, fetch_m3u8_source, M3u8Refresher};
use super::proxy::{fetch_get_json, fetch_get_text};

const SC_BASE_URL: &str = "https://soundcloud.com";
const SC_API_V2: &str = "https://api-v2.soundcloud.com";

#[derive(Debug, serde::Deserialize)]
pub struct TranscodingFormat {
    pub protocol: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Transcoding {
    pub url: String,
    pub preset: Option<String>,
    pub snipped: Option<bool>,
    pub quality: Option<String>,
    pub format: Option<TranscodingFormat>,
}

#[derive(Debug, serde::Deserialize)]
pub struct TrackMedia {
    pub transcodings: Option<Vec<Transcoding>>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ResolvedTrack {
    pub permalink_url: Option<String>,
    pub track_authorization: Option<String>,
    pub media: Option<TrackMedia>,
}

#[derive(Debug, serde::Deserialize)]
struct TranscodingResolveResponse {
    url: String,
}

pub struct AnonStreamResult {
    pub data: Bytes,
    pub content_type: &'static str,
}

/// Shared client_id cache
pub struct AnonClient {
    client: Client,
    proxy_url: String,
    client_id: Arc<RwLock<Option<String>>>,
}

impl AnonClient {
    pub fn new(client: Client, proxy_url: String) -> Self {
        Self {
            client,
            proxy_url,
            client_id: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get_client_id(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        {
            let cached = self.client_id.read().await;
            if let Some(ref id) = *cached {
                return Ok(id.clone());
            }
        }
        self.refresh_client_id().await
    }

    pub async fn get_track_by_id(
        &self,
        track_id: &str,
    ) -> Result<ResolvedTrack, Box<dyn std::error::Error + Send + Sync>> {
        let client_id = self.get_client_id().await?;
        let target = format!("{SC_API_V2}/tracks/{track_id}?client_id={client_id}");

        match fetch_get_json::<ResolvedTrack>(
            &self.client,
            &self.proxy_url,
            &target,
            HashMap::new(),
            false,
        )
        .await
        {
            Ok(t) => Ok(t),
            Err(_) => {
                let new_id = self.invalidate_and_refresh().await?;
                let retry_target = format!("{SC_API_V2}/tracks/{track_id}?client_id={new_id}");
                fetch_get_json(
                    &self.client,
                    &self.proxy_url,
                    &retry_target,
                    HashMap::new(),
                    false,
                )
                .await
            }
        }
    }

    pub async fn resolve_url(
        &self,
        url: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let client_id = self.get_client_id().await?;
        let target = build_resolve_target(url, &client_id);

        match fetch_get_json::<serde_json::Value>(
            &self.client,
            &self.proxy_url,
            &target,
            HashMap::new(),
            false,
        )
        .await
        {
            Ok(track) => Ok(track),
            Err(_) => {
                let new_id = self.invalidate_and_refresh().await?;
                let retry_target = build_resolve_target(url, &new_id);
                fetch_get_json(
                    &self.client,
                    &self.proxy_url,
                    &retry_target,
                    HashMap::new(),
                    false,
                )
                .await
            }
        }
    }

    pub async fn resolve_transcoding_url(
        &self,
        transcoding_url: &str,
        explicit_client_id: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let client_id = match explicit_client_id {
            Some(id) => id.to_string(),
            None => self.get_client_id().await?,
        };
        let sep = if transcoding_url.contains('?') {
            "&"
        } else {
            "?"
        };
        let target = format!("{transcoding_url}{sep}client_id={client_id}");

        match fetch_get_json::<TranscodingResolveResponse>(
            &self.client,
            &self.proxy_url,
            &target,
            HashMap::new(),
            false,
        )
        .await
        {
            Ok(r) => Ok(r.url),
            Err(_) if explicit_client_id.is_none() => {
                let new_id = self.invalidate_and_refresh().await?;
                let retry_target = format!("{transcoding_url}{sep}client_id={new_id}");
                let r: TranscodingResolveResponse = fetch_get_json(
                    &self.client,
                    &self.proxy_url,
                    &retry_target,
                    HashMap::new(),
                    false,
                )
                .await?;
                Ok(r.url)
            }
            Err(e) => Err(e),
        }
    }

    /// Get stream for track via anon API v2
    pub async fn get_stream(
        self: &Arc<Self>,
        track_urn: &str,
    ) -> Result<Option<AnonStreamResult>, Box<dyn std::error::Error + Send + Sync>> {
        let track_id = track_urn.rsplit(':').next().unwrap_or(track_urn);

        let track = match self.get_track_by_id(track_id).await {
            Ok(t) => t,
            Err(e) => {
                warn!("[anon] get track failed: {e}");
                return Ok(None);
            }
        };

        let transcodings = track.media.as_ref().and_then(|m| m.transcodings.as_ref());

        // If no transcodings — refresh client_id and retry track fetch
        let transcodings = match transcodings {
            Some(t) if !t.is_empty() => t,
            _ => {
                warn!("[anon] no transcodings for {track_id}, refreshing client_id");
                self.invalidate_and_refresh().await?;
                let retry_track = match self.get_track_by_id(track_id).await {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("[anon] retry get track failed: {e}");
                        return Ok(None);
                    }
                };
                match retry_track
                    .media
                    .as_ref()
                    .and_then(|m| m.transcodings.as_ref())
                {
                    Some(t) if !t.is_empty() => {
                        // Return immediately from the retry path
                        return self.stream_from_transcodings(t).await;
                    }
                    _ => {
                        warn!("[anon] still no transcodings for {track_id} after refresh");
                        return Ok(None);
                    }
                }
            }
        };

        match self.stream_from_transcodings(transcodings).await {
            Ok(Some(r)) => Ok(Some(r)),
            Ok(None) => Ok(None),
            Err(e) => {
                // Stream failed — refresh client_id and retry
                warn!("[anon] stream failed for {track_id}, refreshing client_id: {e}");
                self.invalidate_and_refresh().await?;
                let retry_track = match self.get_track_by_id(track_id).await {
                    Ok(t) => t,
                    Err(e2) => {
                        warn!("[anon] retry get track failed: {e2}");
                        return Ok(None);
                    }
                };
                match retry_track
                    .media
                    .as_ref()
                    .and_then(|m| m.transcodings.as_ref())
                {
                    Some(t) if !t.is_empty() => self.stream_from_transcodings(t).await,
                    _ => Ok(None),
                }
            }
        }
    }

    async fn stream_from_transcodings(
        self: &Arc<Self>,
        transcodings: &[Transcoding],
    ) -> Result<Option<AnonStreamResult>, Box<dyn std::error::Error + Send + Sync>> {
        let ranked = ranked_transcodings(transcodings);
        if ranked.is_empty() {
            return Ok(None);
        }

        let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
        for t in ranked {
            let mime = t
                .format
                .as_ref()
                .and_then(|f| f.mime_type.as_deref())
                .unwrap_or("audio/mpeg");
            let is_progressive = t
                .format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                == Some("progressive");

            let media_url = match self.resolve_transcoding_url(&t.url, None).await {
                Ok(u) => u,
                Err(e) => {
                    warn!(
                        "[anon] resolve {} failed: {e}",
                        t.preset.as_deref().unwrap_or("?")
                    );
                    last_err = Some(e);
                    continue;
                }
            };

            let result = if is_progressive {
                download_progressive(
                    &self.client,
                    &self.proxy_url,
                    &media_url,
                    mime,
                    HashMap::new(),
                    false,
                )
                .await
            } else {
                let refresher: M3u8Refresher = {
                    let me = Arc::clone(self);
                    let transcoding_url = t.url.clone();
                    Arc::new(move || {
                        let me = Arc::clone(&me);
                        let transcoding_url = transcoding_url.clone();
                        Box::pin(async move {
                            let media = me.resolve_transcoding_url(&transcoding_url, None).await?;
                            fetch_m3u8_source(
                                &me.client,
                                &me.proxy_url,
                                &media,
                                HashMap::new(),
                                false,
                            )
                            .await
                        })
                    })
                };
                download_hls(
                    &self.client,
                    &self.proxy_url,
                    &media_url,
                    mime,
                    HashMap::new(),
                    false,
                    Some(refresher),
                )
                .await
            };

            match result {
                Ok((data, content_type)) => {
                    return Ok(Some(AnonStreamResult { data, content_type }))
                }
                Err(e) => {
                    warn!(
                        "[anon] transcoding {} ({}) failed: {e}",
                        t.preset.as_deref().unwrap_or("?"),
                        if is_progressive { "progressive" } else { "hls" },
                    );
                    last_err = Some(e);
                }
            }
        }

        // All failed — return Err to trigger client_id refresh retry
        Err(last_err.unwrap_or_else(|| "all anon transcodings failed".into()))
    }

    async fn refresh_client_id(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut headers = HashMap::new();
        headers.insert(
            "User-Agent".into(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36".into(),
        );

        let (html, _) =
            fetch_get_text(&self.client, &self.proxy_url, SC_BASE_URL, headers, false).await?;

        let client_id = extract_client_id_from_hydration(&html)
            .ok_or("Failed to extract SoundCloud client_id from page")?;

        let mut cached = self.client_id.write().await;
        *cached = Some(client_id.clone());
        info!("Refreshed SoundCloud public client_id");
        Ok(client_id)
    }

    async fn invalidate_and_refresh(
        &self,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut cached = self.client_id.write().await;
            *cached = None;
        }
        self.refresh_client_id().await
    }
}

/// Return all valid transcodings ranked: progressive first (safest — single
/// GET, no chunk-level failures), then HLS by preset preference.
fn ranked_transcodings(transcodings: &[Transcoding]) -> Vec<&Transcoding> {
    let candidates: Vec<&Transcoding> = transcodings
        .iter()
        .filter(|t| {
            let encrypted = t
                .format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                .unwrap_or("")
                .contains("encrypted");
            !encrypted && !t.snipped.unwrap_or(false) && !t.url.contains("/preview")
        })
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    let is_progressive = |t: &&Transcoding| {
        t.format
            .as_ref()
            .and_then(|f| f.protocol.as_deref())
            == Some("progressive")
    };

    const PRESET_ORDER: &[&str] = &["mp3_1_0", "aac_160k", "opus_0_0", "abr_sq"];

    let mut ordered: Vec<&Transcoding> = Vec::with_capacity(candidates.len());

    // 1. Progressive first (ranked by same preset order)
    for preset in PRESET_ORDER {
        if let Some(t) = candidates
            .iter()
            .find(|t| is_progressive(t) && t.preset.as_deref() == Some(preset))
        {
            ordered.push(t);
        }
    }
    for t in &candidates {
        if is_progressive(t) && !ordered.iter().any(|o| std::ptr::eq(*o, *t)) {
            ordered.push(t);
        }
    }

    // 2. HLS by preset preference
    for preset in PRESET_ORDER {
        if let Some(t) = candidates
            .iter()
            .find(|t| !is_progressive(t) && t.preset.as_deref() == Some(preset))
        {
            ordered.push(t);
        }
    }
    // 3. Any remainder
    for t in &candidates {
        if !ordered.iter().any(|o| std::ptr::eq(*o, *t)) {
            ordered.push(t);
        }
    }
    ordered
}

/// Extract client_id from window.__sc_hydration on SC homepage
fn extract_client_id_from_hydration(html: &str) -> Option<String> {
    let pattern = r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([^"]+)""#;
    let re = regex::Regex::new(pattern).ok()?;
    let caps = re.captures(html)?;
    caps.get(1).map(|m| m.as_str().to_string())
}

fn build_resolve_target(url: &str, client_id: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("url", url)
        .append_pair("client_id", client_id)
        .finish();
    format!("{SC_API_V2}/resolve?{query}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MEDIA: &str = r#"{
        "transcodings": [
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/0e383483-3590-44ee-a4a3-c25df162579f/stream/cbc-encrypted-hls", "preset": "aac_160k", "duration": 148821, "snipped": false, "quality": "sq", "is_legacy_transcoding": false, "format": { "protocol": "cbc-encrypted-hls", "mime_type": "audio/mp4; codecs=\"mp4a.40.2\"" } },
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/0e383483-3590-44ee-a4a3-c25df162579f/stream/ctr-encrypted-hls", "preset": "aac_160k", "duration": 148821, "snipped": false, "quality": "sq", "is_legacy_transcoding": false, "format": { "protocol": "ctr-encrypted-hls", "mime_type": "audio/mp4; codecs=\"mp4a.40.2\"" } },
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/31935ff7-e765-4086-8771-431fe2306b83/stream/cbc-encrypted-hls", "preset": "aac_96k", "duration": 148821, "snipped": false, "quality": "lq", "is_legacy_transcoding": false, "format": { "protocol": "cbc-encrypted-hls", "mime_type": "audio/mp4; codecs=\"mp4a.40.2\"" } },
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/31935ff7-e765-4086-8771-431fe2306b83/stream/ctr-encrypted-hls", "preset": "aac_96k", "duration": 148821, "snipped": false, "quality": "lq", "is_legacy_transcoding": false, "format": { "protocol": "ctr-encrypted-hls", "mime_type": "audio/mp4; codecs=\"mp4a.40.2\"" } },
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/66fd47a6-5e9f-4eb5-ad45-e2a5e6caa04d/stream/cbc-encrypted-hls", "preset": "abr_sq", "duration": 148821, "snipped": false, "quality": "sq", "is_legacy_transcoding": false, "format": { "protocol": "cbc-encrypted-hls", "mime_type": "audio/mpegurl" } },
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/66fd47a6-5e9f-4eb5-ad45-e2a5e6caa04d/stream/ctr-encrypted-hls", "preset": "abr_sq", "duration": 148821, "snipped": false, "quality": "sq", "is_legacy_transcoding": false, "format": { "protocol": "ctr-encrypted-hls", "mime_type": "audio/mpegurl" } },
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/1dc4586b-cb41-46d1-89c9-5aed528f6620/stream/hls", "preset": "mp3_1_0", "duration": 148846, "snipped": false, "quality": "sq", "is_legacy_transcoding": true, "format": { "protocol": "hls", "mime_type": "audio/mpeg" } },
            { "url": "https://api-v2.soundcloud.com/media/soundcloud:tracks:2028682452/1dc4586b-cb41-46d1-89c9-5aed528f6620/stream/progressive", "preset": "mp3_1_0", "duration": 148846, "snipped": false, "quality": "sq", "is_legacy_transcoding": true, "format": { "protocol": "progressive", "mime_type": "audio/mpeg" } }
        ]
    }"#;

    fn protocol(t: &Transcoding) -> &str {
        t.format
            .as_ref()
            .and_then(|f| f.protocol.as_deref())
            .unwrap_or("")
    }

    #[test]
    fn anon_picks_mp3_progressive_first_when_others_encrypted() {
        let media: TrackMedia = serde_json::from_str(SAMPLE_MEDIA).unwrap();
        let transcodings = media.transcodings.expect("transcodings present");

        let ranked = ranked_transcodings(&transcodings);

        // Encrypted (cbc/ctr) variants are filtered out entirely: only the
        // two legacy mp3_1_0 entries survive.
        assert_eq!(
            ranked.len(),
            2,
            "only non-encrypted mp3_1_0 transcodings should remain"
        );
        assert!(
            ranked.iter().all(|t| !protocol(t).contains("encrypted")),
            "no encrypted transcoding may survive the filter"
        );

        // First attempt: progressive mp3_1_0 (single GET, safest).
        let first = ranked[0];
        assert_eq!(protocol(first), "progressive");
        assert_eq!(first.preset.as_deref(), Some("mp3_1_0"));
        assert_eq!(
            first.format.as_ref().and_then(|f| f.mime_type.as_deref()),
            Some("audio/mpeg")
        );

        // Fallback: the mp3_1_0 HLS variant.
        let second = ranked[1];
        assert_eq!(protocol(second), "hls");
        assert_eq!(second.preset.as_deref(), Some("mp3_1_0"));
    }
}
