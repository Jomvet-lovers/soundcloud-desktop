use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tracing::debug;

const LRCLIB_API: &str = "https://lrclib.net/api";
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct LrclibResult {
    pub synced_lrc: Option<String>,
    pub plain_text: Option<String>,
    pub artist_guess: Option<String>,
    pub title_guess: Option<String>,
    pub duration_sec: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Raw {
    #[serde(default, rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default, rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    #[serde(default, rename = "artistName")]
    artist_name: Option<String>,
    #[serde(default, rename = "trackName")]
    track_name: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
}

pub struct LrclibService {
    http: Client,
}

impl LrclibService {
    pub fn new(http: Client) -> Arc<Self> {
        Arc::new(Self { http })
    }

    pub async fn search_by_query(&self, q: &str, limit: usize) -> Vec<LrclibResult> {
        let url = format!("{LRCLIB_API}/search?q={}", urlencoding::encode(q));
        let resp = match self.http.get(&url).timeout(TIMEOUT).send().await {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "LRCLIB search failed");
                return Vec::new();
            }
        };
        if !resp.status().is_success() {
            return Vec::new();
        }
        let data: Vec<Raw> = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                debug!(error = %e, "LRCLIB parse failed");
                return Vec::new();
            }
        };
        data.into_iter()
            .take(limit)
            .filter(|e| e.synced_lyrics.is_some() || e.plain_lyrics.is_some())
            .map(|e| LrclibResult {
                synced_lrc: e.synced_lyrics,
                plain_text: e.plain_lyrics,
                artist_guess: e.artist_name,
                title_guess: e.track_name,
                duration_sec: e.duration.map(|d| d as i64),
            })
            .collect()
    }
}
