use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{debug, info, warn};

use super::anon::{AnonClient, Transcoding};
use super::hls::{download_hls_full, download_progressive};
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

    /// Get stream via cookies.
    /// `hq_only=true`  → only HQ transcodings
    /// `hq_only=false` → all transcodings (HQ → SQ)
    pub async fn get_stream(
        &self,
        track_urn: &str,
        hq_only: bool,
    ) -> Result<Option<CookieStreamResult>, Box<dyn std::error::Error + Send + Sync>> {
        let track_id = track_urn.rsplit(':').next().unwrap_or(track_urn);

        // Get track to find permalink
        let track = self.anon.get_track_by_id(track_id).await?;
        let permalink = match track.permalink_url {
            Some(ref p) => p.clone(),
            None => {
                debug!("[cookies] no permalink for {track_id}");
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
                debug!("[cookies] no transcodings for {track_id}");
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
            debug!("[cookies] no full transcodings for {track_id}");
            return Ok(None);
        }

        // Sort: progressive before HLS within each tier, non-encrypted before encrypted
        let is_encrypted = |t: &&Transcoding| {
            t.format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                .unwrap_or("")
                .contains("encrypted")
        };
        let is_progressive = |t: &&Transcoding| {
            t.format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                == Some("progressive")
        };
        let is_hq = |t: &&Transcoding| t.quality.as_deref() == Some("hq");

        // Tiers (safest first): HQ progressive → HQ hls → HQ enc → SQ progressive → SQ hls → SQ enc
        let mut ordered: Vec<&Transcoding> = Vec::with_capacity(full.len());
        ordered.extend(full.iter().filter(|t| is_hq(t) && is_progressive(t)));
        ordered.extend(
            full.iter()
                .filter(|t| is_hq(t) && !is_progressive(t) && !is_encrypted(t)),
        );
        ordered.extend(full.iter().filter(|t| is_hq(t) && is_encrypted(t)));
        if !hq_only {
            ordered.extend(full.iter().filter(|t| !is_hq(t) && is_progressive(t)));
            ordered.extend(
                full.iter()
                    .filter(|t| !is_hq(t) && !is_progressive(t) && !is_encrypted(t)),
            );
            ordered.extend(full.iter().filter(|t| !is_hq(t) && is_encrypted(t)));
        }

        for transcoding in ordered {
            let quality = if transcoding.quality.as_deref() == Some("hq") {
                "hq"
            } else {
                "sq"
            };

            match self
                .try_transcoding(transcoding, &track_auth, &client_id)
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
                    debug!(
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
        transcoding: &Transcoding,
        track_auth: &str,
        client_id: &str,
    ) -> Result<(Bytes, &'static str), Box<dyn std::error::Error + Send + Sync>> {
        let transcoding_url = &transcoding.url;
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

        let mime = transcoding
            .format
            .as_ref()
            .and_then(|f| f.mime_type.as_deref())
            .unwrap_or("audio/mpeg");
        let is_progressive = transcoding
            .format
            .as_ref()
            .and_then(|f| f.protocol.as_deref())
            == Some("progressive");

        if is_progressive {
            download_progressive(
                &self.client,
                &self.proxy_url,
                &resp.url,
                mime,
                HashMap::new(),
            )
            .await
        } else {
            download_hls_full(
                &self.client,
                &self.proxy_url,
                &resp.url,
                mime,
                HashMap::new(),
            )
            .await
        }
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
        let n = prev + 1;
        // Log at threshold, then every 25 failures to indicate sustained degradation.
        if n == FAILURE_THRESHOLD || (n > FAILURE_THRESHOLD && n % 25 == 0) {
            warn!("[cookies] consecutive failures: {n}");
        } else {
            debug!("[cookies] consecutive failures: {n}");
        }
    }

    fn record_success(&self) {
        let prev = self.consecutive_failures.swap(0, Ordering::Relaxed);
        if prev >= FAILURE_THRESHOLD {
            info!("[cookies] recovered after {prev} failures");
        }
    }
}

/// Extract a balanced JSON object starting from '{', handling nested braces and strings.
fn extract_balanced_json(s: &str) -> Option<&str> {
    if !s.starts_with('{') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;

    for (i, ch) in s.char_indices() {
        if !in_str {
            match ch {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&s[..i + 1]);
                    }
                }
                _ => {}
            }
        } else {
            if ch == '"' && !esc {
                in_str = false;
            }
            esc = !esc && ch == '\\';
        }
    }
    None
}

/// Extract sound + clientId from cookie hydration data
fn extract_cookie_hydration_data(html: &str) -> Option<(CookieHydrationSound, String)> {
    let client_id_pattern =
        r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([^"]+)""#;
    let client_id_re = regex::Regex::new(client_id_pattern).ok()?;
    let client_id = client_id_re.captures(html)?.get(1)?.as_str().to_string();

    let sound_pattern = r#""hydratable"\s*:\s*"sound"\s*,\s*"data"\s*:\s*\{"#;
    let sound_re = regex::Regex::new(sound_pattern).ok()?;
    let sound_match = sound_re.find(html)?;
    // Start from the opening '{' (last char of the match)
    let sound_start = sound_match.end() - 1;
    let rest = &html[sound_start..];

    let sound_json = extract_balanced_json(rest)?;
    let sound: CookieHydrationSound = match serde_json::from_str(sound_json) {
        Ok(s) => s,
        Err(e) => {
            warn!("[cookies] sound JSON parse failed: {e}");
            return None;
        }
    };

    Some((sound, client_id))
}
