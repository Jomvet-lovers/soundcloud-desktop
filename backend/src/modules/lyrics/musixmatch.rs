use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::future::join_all;
use parking_lot_compat::Mutex;
use regex::Regex;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use tracing::debug;

use crate::common::external_fetch::ExternalFetcher;

const APP_ID: &str = "web-desktop-app-v1.0";
const TOKEN_TTL: Duration = Duration::from_secs(9 * 60 * 60);
const UA: &str = "scd-backend/0.1 (musixmatch lookup)";

mod parking_lot_compat {
    use std::sync::Mutex as StdMutex;
    pub type Mutex<T> = StdMutex<T>;
}

#[derive(Debug, Clone)]
pub struct MxmCandidate {
    pub synced_lrc: Option<String>,
    pub plain_text: Option<String>,
    pub artist_guess: Option<String>,
    pub title_guess: Option<String>,
    pub duration_sec: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TokenResp {
    message: Option<TokenMsg>,
}
#[derive(Debug, Deserialize)]
struct TokenMsg {
    body: Option<TokenBody>,
}
#[derive(Debug, Deserialize)]
struct TokenBody {
    user_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResp {
    message: Option<SearchMsg>,
}
#[derive(Debug, Deserialize)]
struct SearchMsg {
    body: Option<SearchBody>,
}
#[derive(Debug, Deserialize)]
struct SearchBody {
    track_list: Option<Vec<TrackListItem>>,
}
#[derive(Debug, Deserialize)]
struct TrackListItem {
    track: Option<Track>,
}
#[derive(Debug, Deserialize)]
struct Track {
    track_id: Option<i64>,
    track_name: Option<String>,
    artist_name: Option<String>,
    track_length: Option<i64>,
    has_lyrics: Option<i64>,
    has_subtitles: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SubtitleResp {
    message: Option<SubtitleMsg>,
}
#[derive(Debug, Deserialize)]
struct SubtitleMsg {
    body: Option<SubtitleBody>,
}
#[derive(Debug, Deserialize)]
struct SubtitleBody {
    subtitle: Option<Subtitle>,
}
#[derive(Debug, Deserialize)]
struct Subtitle {
    subtitle_body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LyricsResp {
    message: Option<LyricsMsg>,
}
#[derive(Debug, Deserialize)]
struct LyricsMsg {
    body: Option<LyricsBody>,
}
#[derive(Debug, Deserialize)]
struct LyricsBody {
    lyrics: Option<Lyrics>,
}
#[derive(Debug, Deserialize)]
struct Lyrics {
    lyrics_body: Option<String>,
}

struct TokenCache {
    token: String,
    expires_at: Instant,
}

pub struct MusixmatchService {
    fetcher: Arc<ExternalFetcher>,
    base: String,
    token_cache: Mutex<Option<TokenCache>>,
}

impl MusixmatchService {
    pub fn new(fetcher: Arc<ExternalFetcher>, base: String) -> Arc<Self> {
        Arc::new(Self {
            fetcher,
            base,
            token_cache: Mutex::new(None),
        })
    }

    fn headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("User-Agent", UA.parse().unwrap());
        h.insert("Accept", "application/json".parse().unwrap());
        h.insert("Cookie", "x-mxm-token-guid=".parse().unwrap());
        h
    }

    pub async fn search_by_query(&self, q: &str, limit: usize) -> Vec<MxmCandidate> {
        let Some(token) = self.get_token().await else {
            return Vec::new();
        };
        let tracks = self.track_search(q, &token, limit).await;
        let token: &str = &token;
        // Параллельно по трекам (а не серийный for{await}) — иначе ~N round-trip
        // подряд в критическом пути лирики.
        let cands = join_all(tracks.into_iter().map(|t| async move {
            let synced_fut = async {
                if t.has_subtitles > 0 {
                    self.subtitle_by_track_id(t.track_id, token).await
                } else {
                    None
                }
            };
            let plain_fut = async {
                if t.has_lyrics > 0 {
                    self.lyrics_by_track_id(t.track_id, token).await
                } else {
                    None
                }
            };
            let (synced, plain) = tokio::join!(synced_fut, plain_fut);
            if synced.is_none() && plain.is_none() {
                return None;
            }
            Some(MxmCandidate {
                synced_lrc: synced,
                plain_text: plain,
                artist_guess: t.artist_name,
                title_guess: t.track_name,
                duration_sec: t.track_length,
            })
        }))
            .await;
        cands.into_iter().flatten().collect()
    }

    async fn track_search(&self, q: &str, token: &str, limit: usize) -> Vec<TrackHit> {
        let url = format!(
            "{}/track.search?app_id={}&usertoken={}&q={}&page_size={}&page=1&format=json",
            self.base,
            APP_ID,
            urlencoding::encode(token),
            urlencoding::encode(q),
            limit,
        );
        let parsed: SearchResp = match self.send_json(&url, "track.search").await {
            Some(p) => p,
            None => return Vec::new(),
        };
        let list = parsed
            .message
            .and_then(|m| m.body)
            .and_then(|b| b.track_list)
            .unwrap_or_default();
        list.into_iter()
            .filter_map(|x| x.track)
            .filter_map(|t| {
                t.track_id.map(|id| TrackHit {
                    track_id: id,
                    track_name: t.track_name,
                    artist_name: t.artist_name,
                    track_length: t.track_length,
                    has_lyrics: t.has_lyrics.unwrap_or(0),
                    has_subtitles: t.has_subtitles.unwrap_or(0),
                })
            })
            .collect()
    }

    async fn subtitle_by_track_id(&self, track_id: i64, token: &str) -> Option<String> {
        let url = format!(
            "{}/track.subtitle.get?app_id={}&usertoken={}&track_id={}&subtitle_format=lrc&format=json",
            self.base,
            APP_ID,
            urlencoding::encode(token),
            track_id,
        );
        let parsed: SubtitleResp = self.send_json(&url, "track.subtitle.get").await?;
        let raw = parsed.message?.body?.subtitle?.subtitle_body?;
        let lrc = normalize_mxm_subtitle(&raw);
        if lrc.len() > 20 {
            Some(lrc)
        } else {
            None
        }
    }

    async fn lyrics_by_track_id(&self, track_id: i64, token: &str) -> Option<String> {
        let url = format!(
            "{}/track.lyrics.get?app_id={}&usertoken={}&track_id={}&format=json",
            self.base,
            APP_ID,
            urlencoding::encode(token),
            track_id,
        );
        let parsed: LyricsResp = self.send_json(&url, "track.lyrics.get").await?;
        let body = parsed.message?.body?.lyrics?.lyrics_body?;
        if body.len() < 20 {
            return None;
        }
        let body_lower = body.to_lowercase();
        if body_lower.contains("this lyrics is not for commercial use") {
            return None;
        }
        let stars: Regex = Regex::new(r"\*{5,}").unwrap();
        if stars.is_match(&body) {
            return None;
        }
        Some(body.trim().to_string())
    }

    async fn get_token(&self) -> Option<String> {
        {
            let g = self.token_cache.lock().ok()?;
            if let Some(c) = g.as_ref() {
                if c.expires_at > Instant::now() {
                    return Some(c.token.clone());
                }
            }
        }
        let url = format!("{}/token.get?app_id={}", self.base, APP_ID);
        let parsed: TokenResp = self.send_json(&url, "token.get").await?;
        let token = parsed.message?.body?.user_token?;
        if token.is_empty() || token == "UpgradeOnlyUpgradeOnlyUpgradeOnlyUpgradeOnly" {
            return None;
        }
        if let Ok(mut g) = self.token_cache.lock() {
            *g = Some(TokenCache {
                token: token.clone(),
                expires_at: Instant::now() + TOKEN_TTL,
            });
        }
        Some(token)
    }

    async fn send_json<T: for<'de> Deserialize<'de>>(&self, url: &str, label: &str) -> Option<T> {
        let bytes = match self.fetcher.get_bytes(url, Self::headers()).await {
            Ok(b) => b,
            Err(e) => {
                debug!(error = %e, label, "mxm request failed");
                return None;
            }
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(p) => Some(p),
            Err(e) => {
                debug!(error = %e, label, "mxm parse failed");
                None
            }
        }
    }
}

struct TrackHit {
    track_id: i64,
    track_name: Option<String>,
    artist_name: Option<String>,
    track_length: Option<i64>,
    has_lyrics: i64,
    has_subtitles: i64,
}

fn normalize_mxm_subtitle(raw: &str) -> String {
    #[derive(Deserialize)]
    struct Line {
        text: Option<String>,
        time: Option<TimeT>,
    }
    #[derive(Deserialize)]
    struct TimeT {
        total: Option<f64>,
    }
    let parsed: Result<Vec<Line>, _> = serde_json::from_str(raw);
    let Ok(lines) = parsed else {
        return raw.to_string();
    };
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let total = line.time.and_then(|t| t.total).unwrap_or(0.0);
        let m = (total / 60.0).floor() as i64;
        let s = total % 60.0;
        let m_str = format!("{:02}", m);
        let s_str = format!("{:05.2}", s);
        let text = line.text.unwrap_or_default();
        out.push(format!("[{m_str}:{s_str}] {text}"));
    }
    out.join("\n")
}
