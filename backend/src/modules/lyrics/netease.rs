use std::sync::Arc;

use futures::future::join_all;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use tracing::debug;

use crate::common::external_fetch::ExternalFetcher;

const SEARCH_LIMIT: usize = 5;
const UA: &str = "scd-backend/0.1 (netease lookup)";

static RE_LRC_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[[^\]\r\n]*\]\s*").unwrap());

#[derive(Debug, Clone)]
pub struct NeteaseResult {
    pub synced_lrc: Option<String>,
    pub plain_text: Option<String>,
    pub artist_guess: Option<String>,
    pub title_guess: Option<String>,
    pub duration_sec: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SearchResp {
    result: Option<SearchResult>,
}
#[derive(Debug, Deserialize)]
struct SearchResult {
    songs: Option<Vec<NcmSong>>,
}
#[derive(Debug, Deserialize, Clone)]
struct NcmArtist {
    name: Option<String>,
}
#[derive(Debug, Deserialize, Clone)]
struct NcmSong {
    id: Option<i64>,
    name: Option<String>,
    duration: Option<i64>,
    dt: Option<i64>,
    artists: Option<Vec<NcmArtist>>,
    ar: Option<Vec<NcmArtist>>,
}

#[derive(Debug, Deserialize)]
struct LyricResp {
    lrc: Option<LyricInner>,
    tlyric: Option<LyricInner>,
}
#[derive(Debug, Deserialize)]
struct LyricInner {
    lyric: Option<String>,
}

pub struct NeteaseService {
    fetcher: Arc<ExternalFetcher>,
    base: String,
}

impl NeteaseService {
    pub fn new(fetcher: Arc<ExternalFetcher>, base: String) -> Arc<Self> {
        Arc::new(Self { fetcher, base })
    }

    fn headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("User-Agent", UA.parse().unwrap());
        h.insert("Accept", "application/json".parse().unwrap());
        h
    }

    pub async fn search_by_query(&self, q: &str, limit: usize) -> Vec<NeteaseResult> {
        let limit = limit.max(1).min(SEARCH_LIMIT);
        let songs = self.search(q, limit).await;
        if songs.is_empty() {
            return Vec::new();
        }
        let lyrics_futs = songs
            .iter()
            .map(|s| self.fetch_lrc(s.id.unwrap_or(0)))
            .collect::<Vec<_>>();
        let lyrics_res = join_all(lyrics_futs).await;
        let mut out = Vec::new();
        for (song, lyric) in songs.into_iter().zip(lyrics_res.into_iter()) {
            let Some(lrc) = lyric else { continue };
            let artists = song.artists.clone().or(song.ar.clone()).unwrap_or_default();
            let artist_guess: String = artists
                .iter()
                .filter_map(|a| a.name.as_deref())
                .collect::<Vec<_>>()
                .join(", ");
            let ms = song.duration.or(song.dt).unwrap_or(0);
            let duration_sec = if ms > 0 {
                Some((ms as f64 / 1000.0).round() as i64)
            } else {
                None
            };
            out.push(NeteaseResult {
                synced_lrc: lrc.synced,
                plain_text: lrc.plain,
                artist_guess: if artist_guess.is_empty() {
                    None
                } else {
                    Some(artist_guess)
                },
                title_guess: song.name,
                duration_sec,
            });
        }
        out
    }

    async fn search(&self, q: &str, limit: usize) -> Vec<NcmSong> {
        let url = format!(
            "{}/search?keywords={}&type=1&limit={}",
            self.base,
            urlencoding::encode(q),
            limit
        );
        let bytes = match self.fetcher.get_bytes(&url, Self::headers()).await {
            Ok(b) => b,
            Err(e) => {
                debug!(error = %e, "netease search failed");
                return Vec::new();
            }
        };
        let parsed: SearchResp = match serde_json::from_slice(&bytes) {
            Ok(p) => p,
            Err(e) => {
                debug!(error = %e, "netease parse failed");
                return Vec::new();
            }
        };
        let songs = parsed.result.and_then(|r| r.songs).unwrap_or_default();
        songs
            .into_iter()
            .filter(|s| s.id.is_some())
            .take(limit)
            .collect()
    }

    async fn fetch_lrc(&self, id: i64) -> Option<Lrc> {
        if id == 0 {
            return None;
        }
        let url = format!("{}/lyric?id={}", self.base, id);
        let bytes = match self.fetcher.get_bytes(&url, Self::headers()).await {
            Ok(b) => b,
            Err(e) => {
                debug!(id, error = %e, "netease lyric failed");
                return None;
            }
        };
        let parsed: LyricResp = match serde_json::from_slice(&bytes) {
            Ok(p) => p,
            Err(e) => {
                debug!(id, error = %e, "netease lyric parse failed");
                return None;
            }
        };
        let synced_raw = parsed
            .lrc
            .and_then(|l| l.lyric)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let plain_src = match &synced_raw {
            Some(s) => Some(s.clone()),
            None => parsed
                .tlyric
                .and_then(|l| l.lyric)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        };
        let plain = plain_src.as_deref().map(strip_lrc_timestamps);
        if synced_raw.is_none() && plain.is_none() {
            return None;
        }
        Some(Lrc {
            synced: synced_raw,
            plain,
        })
    }
}

struct Lrc {
    synced: Option<String>,
    plain: Option<String>,
}

fn strip_lrc_timestamps(lrc: &str) -> String {
    lrc.split('\n')
        .map(|line| RE_LRC_TAG.replace_all(line, "").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
