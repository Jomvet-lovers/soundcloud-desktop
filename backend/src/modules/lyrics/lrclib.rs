use std::sync::Arc;

use reqwest::header::HeaderMap;
use serde::Deserialize;
use tracing::debug;

use crate::common::external_fetch::ExternalFetcher;

const LRCLIB_API: &str = "https://lrclib.net/api";
const UA: &str = "scd-backend/0.1 (lrclib lookup)";

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
    fetcher: Arc<ExternalFetcher>,
}

impl LrclibService {
    pub fn new(fetcher: Arc<ExternalFetcher>) -> Arc<Self> {
        Arc::new(Self { fetcher })
    }

    fn headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("User-Agent", UA.parse().unwrap());
        h.insert("Accept", "application/json".parse().unwrap());
        h
    }

    pub async fn search_by_query(&self, q: &str, limit: usize) -> Vec<LrclibResult> {
        let url = format!("{LRCLIB_API}/search?q={}", urlencoding::encode(q));
        let bytes = match self.fetcher.get_bytes(&url, Self::headers()).await {
            Ok(b) => b,
            Err(e) => {
                debug!(error = %e, "LRCLIB search failed");
                return Vec::new();
            }
        };
        let data: Vec<Raw> = match serde_json::from_slice(&bytes) {
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
