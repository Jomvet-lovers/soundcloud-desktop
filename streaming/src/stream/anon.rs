use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::hls::download_hls_full;
use super::proxy::{proxy_get_json, proxy_get_text};

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

        match proxy_get_json::<ResolvedTrack>(
            &self.client,
            &self.proxy_url,
            &target,
            HashMap::new(),
        )
        .await
        {
            Ok(t) => Ok(t),
            Err(_) => {
                let new_id = self.invalidate_and_refresh().await?;
                let retry_target = format!("{SC_API_V2}/tracks/{track_id}?client_id={new_id}");
                proxy_get_json(&self.client, &self.proxy_url, &retry_target, HashMap::new()).await
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

        match proxy_get_json::<TranscodingResolveResponse>(
            &self.client,
            &self.proxy_url,
            &target,
            HashMap::new(),
        )
        .await
        {
            Ok(r) => Ok(r.url),
            Err(_) if explicit_client_id.is_none() => {
                let new_id = self.invalidate_and_refresh().await?;
                let retry_target = format!("{transcoding_url}{sep}client_id={new_id}");
                let r: TranscodingResolveResponse =
                    proxy_get_json(&self.client, &self.proxy_url, &retry_target, HashMap::new())
                        .await?;
                Ok(r.url)
            }
            Err(e) => Err(e),
        }
    }

    /// Get stream for track via anon API v2
    pub async fn get_stream(
        &self,
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

        let transcodings = match transcodings {
            Some(t) if !t.is_empty() => t,
            _ => {
                warn!("[anon] no transcodings for {track_id}");
                return Ok(None);
            }
        };

        let transcoding = match pick_transcoding(transcodings) {
            Some(t) => t,
            None => return Ok(None),
        };

        let mime = transcoding
            .format
            .as_ref()
            .and_then(|f| f.mime_type.as_deref())
            .unwrap_or("audio/mpeg");

        let m3u8_url = self.resolve_transcoding_url(&transcoding.url, None).await?;
        let (data, content_type) =
            download_hls_full(&self.client, &self.proxy_url, &m3u8_url, mime).await?;

        Ok(Some(AnonStreamResult { data, content_type }))
    }

    async fn refresh_client_id(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut headers = HashMap::new();
        headers.insert(
            "User-Agent".into(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36".into(),
        );

        let (html, _) = proxy_get_text(&self.client, &self.proxy_url, SC_BASE_URL, headers).await?;

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

/// Pick best transcoding: non-encrypted, non-snipped, non-preview
fn pick_transcoding(transcodings: &[Transcoding]) -> Option<&Transcoding> {
    let candidates: Vec<&Transcoding> = transcodings
        .iter()
        .filter(|t| {
            let encrypted = t
                .format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                .unwrap_or("")
                .contains("encrypted");
            let snipped = t.snipped.unwrap_or(false);
            let preview = t.url.contains("/preview");
            !encrypted && !snipped && !preview
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    const PRESET_ORDER: &[&str] = &["mp3_1_0", "aac_160k", "opus_0_0", "abr_sq"];

    for preset in PRESET_ORDER {
        if let Some(t) = candidates
            .iter()
            .find(|t| t.preset.as_deref() == Some(preset))
        {
            return Some(t);
        }
    }

    Some(candidates[0])
}

/// Extract client_id from window.__sc_hydration on SC homepage
fn extract_client_id_from_hydration(html: &str) -> Option<String> {
    let pattern = r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([^"]+)""#;
    let re = regex::Regex::new(pattern).ok()?;
    let caps = re.captures(html)?;
    caps.get(1).map(|m| m.as_str().to_string())
}
