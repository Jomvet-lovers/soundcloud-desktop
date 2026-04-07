use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{info, warn};

use super::anon::{AnonClient, Transcoding};
use super::hls::download_hls_full;
use super::proxy::{proxy_get_json, proxy_get_text};

const FAILURE_THRESHOLD: u32 = 3;

pub struct CookieStreamResult {
    pub data: Bytes,
    pub content_type: &'static str,
    pub quality: &'static str, // "hq" or "sq"
}

pub struct CookiesClient {
    client: Client,
    proxy_url: String,
    cookies: String,
    oauth_token: String,
    anon: AnonClient,
    consecutive_failures: AtomicU32,
}

#[derive(Debug, serde::Deserialize)]
struct CookieHydrationSound {
    media: Option<CookieHydrationMedia>,
    track_authorization: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CookieHydrationMedia {
    transcodings: Option<Vec<Transcoding>>,
}

impl CookiesClient {
    pub fn new(
        client: Client,
        proxy_url: String,
        cookies: String,
        oauth_token: String,
        anon: AnonClient,
    ) -> Self {
        Self {
            client,
            proxy_url,
            cookies,
            oauth_token,
            anon,
            consecutive_failures: AtomicU32::new(0),
        }
    }

    /// Get HQ stream via cookies. Returns None if cookies not available or all transcodings fail.
    pub async fn get_stream(
        &self,
        track_urn: &str,
    ) -> Result<Option<CookieStreamResult>, Box<dyn std::error::Error + Send + Sync>> {
        let track_id = track_urn.rsplit(':').next().unwrap_or(track_urn);

        // Get track to find permalink
        let track = self.anon.get_track_by_id(track_id).await?;
        let permalink = match track.permalink_url {
            Some(ref p) => p.clone(),
            None => {
                warn!("[cookies] no permalink for {track_id}");
                return Ok(None);
            }
        };

        // Fetch page with cookies → extract hydration
        let (sound, client_id) = match self.fetch_hydration(&permalink).await {
            Some((s, c)) => (s, c),
            None => return Ok(None),
        };

        let transcodings = match sound.media.and_then(|m| m.transcodings) {
            Some(t) if !t.is_empty() => t,
            _ => {
                warn!("[cookies] no transcodings for {track_id}");
                self.record_failure();
                return Ok(None);
            }
        };

        let track_auth = sound.track_authorization.unwrap_or_default();

        // Filter non-snippet, non-preview
        let full: Vec<&Transcoding> = transcodings
            .iter()
            .filter(|t| !t.snipped.unwrap_or(false) && !t.url.contains("/preview"))
            .collect();

        if full.is_empty() {
            warn!("[cookies] no full transcodings for {track_id}");
            return Ok(None);
        }

        // Sort: HQ non-encrypted → HQ encrypted → SQ non-encrypted → SQ encrypted
        let is_encrypted = |t: &&Transcoding| {
            t.format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                .unwrap_or("")
                .contains("encrypted")
        };

        let mut ordered: Vec<&Transcoding> = Vec::with_capacity(full.len());
        // HQ non-encrypted
        ordered.extend(
            full.iter()
                .filter(|t| t.quality.as_deref() == Some("hq") && !is_encrypted(t)),
        );
        // HQ encrypted
        ordered.extend(
            full.iter()
                .filter(|t| t.quality.as_deref() == Some("hq") && is_encrypted(t)),
        );
        // SQ non-encrypted
        ordered.extend(
            full.iter()
                .filter(|t| t.quality.as_deref() != Some("hq") && !is_encrypted(t)),
        );
        // SQ encrypted
        ordered.extend(
            full.iter()
                .filter(|t| t.quality.as_deref() != Some("hq") && is_encrypted(t)),
        );

        for transcoding in ordered {
            let quality = if transcoding.quality.as_deref() == Some("hq") {
                "hq"
            } else {
                "sq"
            };
            let mime = transcoding
                .format
                .as_ref()
                .and_then(|f| f.mime_type.as_deref())
                .unwrap_or("audio/mpeg");

            match self
                .try_transcoding(&transcoding.url, &track_auth, &client_id, mime)
                .await
            {
                Ok((data, content_type)) => {
                    self.record_success();
                    return Ok(Some(CookieStreamResult {
                        data,
                        content_type,
                        quality,
                    }));
                }
                Err(e) => {
                    warn!(
                        "[cookies] transcoding {} failed: {e}",
                        transcoding.preset.as_deref().unwrap_or("?")
                    );
                }
            }
        }

        self.record_failure();
        Ok(None)
    }

    async fn try_transcoding(
        &self,
        transcoding_url: &str,
        track_auth: &str,
        client_id: &str,
        mime: &str,
    ) -> Result<(Bytes, &'static str), Box<dyn std::error::Error + Send + Sync>> {
        let sep = if transcoding_url.contains('?') {
            "&"
        } else {
            "?"
        };
        let target =
            format!("{transcoding_url}{sep}client_id={client_id}&track_authorization={track_auth}");

        let mut headers = HashMap::new();
        headers.insert("Accept".into(), "*/*".into());
        headers.insert(
            "Authorization".into(),
            format!("OAuth {}", self.oauth_token),
        );
        headers.insert("Origin".into(), "https://soundcloud.com".into());
        headers.insert("Referer".into(), "https://soundcloud.com/".into());
        headers.insert(
            "User-Agent".into(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36".into(),
        );

        #[derive(serde::Deserialize)]
        struct ResolveResp {
            url: String,
        }

        let resp: ResolveResp =
            proxy_get_json(&self.client, &self.proxy_url, &target, headers).await?;
        download_hls_full(&self.client, &self.proxy_url, &resp.url, mime).await
    }

    async fn fetch_hydration(&self, permalink_url: &str) -> Option<(CookieHydrationSound, String)> {
        let mut headers = HashMap::new();
        headers.insert(
            "User-Agent".into(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36".into(),
        );
        headers.insert("Cookie".into(), self.cookies.clone());

        let (html, _) = proxy_get_text(&self.client, &self.proxy_url, permalink_url, headers)
            .await
            .ok()?;

        extract_cookie_hydration_data(&html)
    }

    fn record_failure(&self) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= FAILURE_THRESHOLD {
            warn!("[cookies] consecutive failures: {}", prev + 1);
        }
    }

    fn record_success(&self) {
        let prev = self.consecutive_failures.swap(0, Ordering::Relaxed);
        if prev >= FAILURE_THRESHOLD {
            info!("[cookies] recovered after {prev} failures");
        }
    }
}

/// Extract sound + clientId from cookie hydration data
fn extract_cookie_hydration_data(html: &str) -> Option<(CookieHydrationSound, String)> {
    let marker = "window.__sc_hydration =";
    let idx = html.find(marker)?;
    let rest = &html[idx + marker.len()..];
    let arr_start = rest.find('[')?;
    let json_start = &rest[arr_start..];

    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut esc = false;
    let mut end_idx = 0;

    for (i, ch) in json_start.chars().enumerate() {
        if !in_str {
            match ch {
                '"' if !esc => in_str = true,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end_idx = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        } else if ch == '"' && !esc {
            in_str = false;
        }
        esc = !esc && ch == '\\';
    }

    if end_idx == 0 {
        return None;
    }

    let entries: Vec<serde_json::Value> = serde_json::from_str(&json_start[..end_idx]).ok()?;

    let mut sound: Option<CookieHydrationSound> = None;
    let mut client_id: Option<String> = None;

    for entry in entries.iter().rev() {
        let hydratable = entry.get("hydratable")?.as_str()?;
        match hydratable {
            "sound" if sound.is_none() => {
                sound = entry
                    .get("data")
                    .and_then(|d| serde_json::from_value(d.clone()).ok());
            }
            "apiClient" if client_id.is_none() => {
                client_id = entry
                    .get("data")
                    .and_then(|d| d.get("id"))
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string());
            }
            _ => {}
        }
    }

    Some((sound?, client_id?))
}
