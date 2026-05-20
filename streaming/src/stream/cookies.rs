use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{debug, info, warn};

use std::sync::Arc;

use super::anon::AnonClient;
use super::hls::{download_hls, download_progressive, fetch_m3u8_source, M3u8Refresher};
use super::proxy::{fetch_get_json, fetch_get_text};
use super::restricted::Transcoding;

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

#[derive(serde::Deserialize)]
struct ResolveResp {
    url: String,
}

#[derive(serde::Deserialize)]
struct AuthedTrack {
    permalink_url: Option<String>,
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

        // Sort: progressive before HLS within each tier, plain before restricted
        let is_encrypted = |t: &&Transcoding| {
            t.format
                .as_ref()
                .and_then(|f| f.protocol.as_deref())
                .unwrap_or("")
                .contains("encrypted")
        };
        let is_progressive = |t: &&Transcoding| {
            t.format.as_ref().and_then(|f| f.protocol.as_deref()) == Some("progressive")
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

    fn auth_headers(&self) -> HashMap<String, String> {
        let mut h = HashMap::new();
        h.insert("Accept".into(), "*/*".into());
        h.insert(
            "Authorization".into(),
            format!("OAuth {}", self.oauth_token),
        );
        h.insert("Origin".into(), "https://soundcloud.com".into());
        h.insert("Referer".into(), "https://soundcloud.com/".into());
        h.insert(
            "User-Agent".into(),
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36".into(),
        );
        h
    }

    /// Authenticated resolve of the `ctr` transcoding (cookies session may
    /// succeed where anon is region-blocked / 404s).
    pub(crate) fn cookie_auth_headers(&self) -> HashMap<String, String> {
        self.auth_headers()
    }

    /// `(transcodings, track_authorization, client_id)` — из cookies-сессии
    /// (через permalink + hydration на soundcloud.com).
    pub(crate) async fn fetch_track_meta(
        &self,
        track_urn: &str,
    ) -> Result<(Vec<Transcoding>, Option<String>, String), Box<dyn std::error::Error + Send + Sync>>
    {
        let track_id = track_urn.rsplit(':').next().unwrap_or(track_urn);
        let track: AuthedTrack = fetch_get_json(
            &self.client,
            &self.proxy_url,
            &format!("https://api-v2.soundcloud.com/tracks/{track_id}"),
            self.auth_headers(),
            false,
        )
        .await?;
        let permalink = track.permalink_url.ok_or("cookies: no permalink")?;
        let (sound, client_id) = self
            .fetch_hydration(&permalink)
            .await
            .ok_or("cookies: hydration failed")?;
        let tcs = sound
            .media
            .and_then(|m| m.transcodings)
            .ok_or("cookies: no transcodings")?;
        Ok((tcs, sound.track_authorization, client_id))
    }

    pub(crate) async fn resolve_restricted(
        &self,
        track_urn: &str,
        hq_first: bool,
    ) -> Result<Option<super::restricted::RestrictedSource>, Box<dyn std::error::Error + Send + Sync>>
    {
        let track_id = track_urn.rsplit(':').next().unwrap_or(track_urn);
        let track: AuthedTrack = fetch_get_json(
            &self.client,
            &self.proxy_url,
            &format!("https://api-v2.soundcloud.com/tracks/{track_id}"),
            self.auth_headers(),
            false,
        )
        .await?;
        let permalink = match track.permalink_url {
            Some(p) => p,
            None => return Ok(None),
        };
        let (sound, client_id) = match self.fetch_hydration(&permalink).await {
            Some(v) => v,
            None => return Ok(None),
        };
        let tcs = match sound.media.and_then(|m| m.transcodings) {
            Some(t) if !t.is_empty() => t,
            _ => return Ok(None),
        };
        super::restricted::resolve(
            &self.client,
            &self.proxy_url,
            &tcs,
            &client_id,
            sound.track_authorization.as_deref(),
            self.auth_headers(),
            hq_first,
        )
        .await
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

        let headers = self.auth_headers();

        let resp: ResolveResp = fetch_get_json(
            &self.client,
            &self.proxy_url,
            &target,
            headers.clone(),
            false,
        )
        .await?;

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
                false,
            )
            .await
        } else {
            // Re-resolve the transcoding (fresh client_id-signed playlist)
            // when segment tokens expire mid-stream.
            let refresher: M3u8Refresher = {
                let client = self.client.clone();
                let proxy_url = self.proxy_url.clone();
                let target = target.clone();
                let headers = headers.clone();
                Arc::new(move || {
                    let client = client.clone();
                    let proxy_url = proxy_url.clone();
                    let target = target.clone();
                    let headers = headers.clone();
                    Box::pin(async move {
                        let resp: ResolveResp =
                            fetch_get_json(&client, &proxy_url, &target, headers, false).await?;
                        fetch_m3u8_source(&client, &proxy_url, &resp.url, HashMap::new(), false)
                            .await
                    })
                })
            };
            download_hls(
                &self.client,
                &self.proxy_url,
                &resp.url,
                mime,
                HashMap::new(),
                false,
                Some(refresher),
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

        let (html, _) =
            fetch_get_text(&self.client, &self.proxy_url, permalink_url, headers, false)
                .await
                .ok()?;

        extract_cookie_hydration_data(&html)
    }

    fn record_failure(&self) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        let n = prev + 1;
        // Log at threshold, then every 25 failures to indicate sustained degradation.
        if n == FAILURE_THRESHOLD || (n > FAILURE_THRESHOLD && n.is_multiple_of(25)) {
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
