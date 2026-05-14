use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use tracing::debug;

use crate::common::external_fetch::ExternalFetcher;
use crate::common::throttle::Throttle;
use crate::config::GeniusCfg;

const GENIUS_API: &str = "https://api.genius.com";
const GENIUS_WEB_API: &str = "https://genius.com/api";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

const API_THROTTLE_MS: u64 = 500;
const WEB_DIRECT_THROTTLE_MS: u64 = 3000;

static RE_OPEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)<div\b[^>]*\bdata-lyrics-container="true"[^>]*>"#).unwrap());
static RE_BR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
static RE_TAGS: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
static RE_LEAD_CONTRIB: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\d+\s*Contributors").unwrap());
static RE_LEAD_LYRICS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^[^\n]*?Lyrics").unwrap());
static RE_LEAD_TEXT_PESN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^\[Текст песни.*?\]").unwrap());

#[derive(Debug, Clone)]
pub struct GeniusCandidate {
    pub plain_text: String,
    pub artist_guess: Option<String>,
    pub title_guess: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResp {
    response: Option<SearchRespBody>,
}
#[derive(Debug, Deserialize)]
struct SearchRespBody {
    sections: Option<Vec<SearchSection>>,
}
#[derive(Debug, Deserialize)]
struct SearchSection {
    #[serde(rename = "type")]
    type_: String,
    hits: Option<Vec<SearchHit>>,
}
#[derive(Debug, Deserialize)]
struct SearchHit {
    result: Option<SearchHitResult>,
}
#[derive(Debug, Deserialize)]
struct SearchHitResult {
    #[serde(default)]
    id: Option<i64>,
    url: Option<String>,
    title: Option<String>,
    primary_artist: Option<PrimaryArtist>,
    #[serde(default)]
    featured_artists: Option<Vec<PrimaryArtist>>,
}

#[derive(Debug, Deserialize)]
struct ApiSearchResp {
    #[serde(default)]
    response: Option<ApiSearchBody>,
}

#[derive(Debug, Deserialize)]
struct ApiSearchBody {
    #[serde(default)]
    hits: Option<Vec<SearchHit>>,
}
#[derive(Debug, Deserialize)]
struct PrimaryArtist {
    #[serde(default)]
    id: Option<i64>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtistSongsResp {
    #[serde(default)]
    response: Option<ArtistSongsBody>,
}

#[derive(Debug, Deserialize)]
struct ArtistSongsBody {
    #[serde(default)]
    songs: Option<Vec<ArtistSong>>,
}

#[derive(Debug, Deserialize)]
struct ArtistSong {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    primary_artist: Option<PrimaryArtist>,
    #[serde(default)]
    featured_artists: Option<Vec<PrimaryArtist>>,
}

#[derive(Debug, Deserialize)]
struct ArtistAlbumsResp {
    #[serde(default)]
    response: Option<ArtistAlbumsBody>,
}

#[derive(Debug, Deserialize)]
struct ArtistAlbumsBody {
    #[serde(default)]
    albums: Option<Vec<AlbumPayload>>,
    #[serde(default)]
    next_page: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AlbumTracksResp {
    #[serde(default)]
    response: Option<AlbumTracksBody>,
}

#[derive(Debug, Deserialize)]
struct AlbumTracksBody {
    #[serde(default)]
    tracks: Option<Vec<AlbumTrackEntry>>,
    #[serde(default)]
    next_page: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AlbumTrackEntry {
    #[serde(default)]
    number: Option<i32>,
    #[serde(default)]
    song: Option<AlbumTrackSong>,
}

#[derive(Debug, Deserialize)]
struct AlbumTrackSong {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    primary_artist: Option<PrimaryArtist>,
    #[serde(default)]
    featured_artists: Option<Vec<PrimaryArtist>>,
}

#[derive(Debug, Deserialize)]
struct ArtistResp {
    #[serde(default)]
    response: Option<ArtistRespBody>,
}

#[derive(Debug, Deserialize)]
struct ArtistRespBody {
    #[serde(default)]
    artist: Option<ArtistPayload>,
}

#[derive(Debug, Deserialize)]
struct ArtistPayload {
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    instagram_name: Option<String>,
    #[serde(default)]
    twitter_name: Option<String>,
    #[serde(default)]
    facebook_name: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SongResp {
    #[serde(default)]
    response: Option<SongRespBody>,
}

#[derive(Debug, Deserialize)]
struct SongRespBody {
    #[serde(default)]
    song: Option<SongPayload>,
}

#[derive(Debug, Deserialize)]
struct SongPayload {
    #[serde(default)]
    album: Option<AlbumPayload>,
    #[serde(default)]
    release_date_components: Option<ReleaseDate>,
}

#[derive(Debug, Deserialize)]
struct AlbumPayload {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    cover_art_url: Option<String>,
    #[serde(default)]
    release_date_components: Option<ReleaseDate>,
}

#[derive(Debug, Deserialize)]
struct ReleaseDate {
    #[serde(default)]
    year: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct GeniusArtistRef {
    pub genius_artist_id: Option<i64>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct GeniusSongMeta {
    pub genius_song_id: Option<i64>,
    pub title: String,
    pub primary_artist: Option<GeniusArtistRef>,
    pub featured: Vec<GeniusArtistRef>,
}

#[derive(Debug, Clone)]
pub struct GeniusArtistDetails {
    pub avatar_url: Option<String>,
    pub instagram: Option<String>,
    pub twitter: Option<String>,
    pub facebook: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GeniusAlbumRef {
    pub genius_album_id: i64,
    pub name: String,
    pub year: Option<i16>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GeniusSongDetails {
    pub album: Option<GeniusAlbumRef>,
    pub year: Option<i16>,
}

#[derive(Debug, Clone)]
pub struct GeniusAlbumTrack {
    pub genius_song_id: i64,
    pub title: String,
    pub position: Option<i32>,
    pub primary_artist: Option<GeniusArtistRef>,
    pub featured: Vec<GeniusArtistRef>,
}

pub struct GeniusService {
    fetcher: Arc<ExternalFetcher>,
    cfg: GeniusCfg,
    api_throttle: Arc<Throttle>,
    web_throttle: Arc<Throttle>,
}

impl GeniusService {
    pub fn new(fetcher: Arc<ExternalFetcher>, cfg: GeniusCfg) -> Arc<Self> {
        Arc::new(Self {
            fetcher,
            cfg,
            api_throttle: Throttle::new(Duration::from_millis(API_THROTTLE_MS)),
            web_throttle: Throttle::new(Duration::from_millis(WEB_DIRECT_THROTTLE_MS)),
        })
    }

    fn has_token(&self) -> bool {
        !self.cfg.access_token.is_empty()
    }

    fn json_headers(&self, with_bearer: bool) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("User-Agent", UA.parse().unwrap());
        h.insert("Accept", "application/json".parse().unwrap());
        if with_bearer && self.has_token() {
            if let Ok(v) = format!("Bearer {}", self.cfg.access_token).parse() {
                h.insert("Authorization", v);
            }
        }
        h
    }

    fn html_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("User-Agent", UA.parse().unwrap());
        h
    }

    fn web_api(&self, path: &str) -> String {
        format!("{GENIUS_WEB_API}{path}")
    }

    fn api(&self, path: &str) -> String {
        format!("{GENIUS_API}{path}")
    }

    async fn fetch_json<T>(&self, url: &str, label: &str) -> Option<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let with_bearer = url.starts_with(GENIUS_API);
        let headers = self.json_headers(with_bearer);
        let result = if with_bearer {
            self.fetcher.get_api(url, headers, &self.api_throttle).await
        } else {
            self.fetcher
                .get_scrape(url, headers, &self.web_throttle)
                .await
        };
        let bytes = match result {
            Ok(b) => b,
            Err(e) => {
                debug!(url, label, error = %e, "genius fetch failed");
                return None;
            }
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(d) => Some(d),
            Err(e) => {
                debug!(url, label, error = %e, "genius parse failed");
                None
            }
        }
    }

    async fn fetch_html(&self, url: &str) -> Option<String> {
        let bytes = match self
            .fetcher
            .get_scrape(url, self.html_headers(), &self.web_throttle)
            .await
        {
            Ok(b) => b,
            Err(e) => {
                debug!(url, error = %e, "genius html fetch failed");
                return None;
            }
        };
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub async fn list_artist_songs(
        &self,
        genius_id: i64,
        page: u32,
        per_page: u32,
    ) -> Vec<GeniusSongMeta> {
        let per = per_page.clamp(1, 50);
        let pg = page.max(1);
        let path = format!("/artists/{genius_id}/songs?per_page={per}&page={pg}&sort=popularity");
        let url = if self.has_token() {
            self.api(&path)
        } else {
            self.web_api(&path)
        };
        let parsed: ArtistSongsResp = match self.fetch_json(&url, "artist songs").await {
            Some(d) => d,
            None => return Vec::new(),
        };
        parsed
            .response
            .map(|r| r.songs.unwrap_or_default())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|s| {
                let title = s.title?;
                let primary = s.primary_artist.as_ref().and_then(map_artist);
                let featured = s
                    .featured_artists
                    .as_deref()
                    .map(|arr| arr.iter().filter_map(map_artist).collect())
                    .unwrap_or_default();
                Some(GeniusSongMeta {
                    genius_song_id: s.id,
                    title,
                    primary_artist: primary,
                    featured,
                })
            })
            .collect()
    }

    pub async fn list_artist_albums(
        &self,
        genius_id: i64,
        page: u32,
        per_page: u32,
    ) -> (Vec<GeniusAlbumRef>, bool) {
        let per = per_page.clamp(1, 50);
        let pg = page.max(1);
        let url = self.web_api(&format!(
            "/artists/{genius_id}/albums?per_page={per}&page={pg}"
        ));
        let parsed: ArtistAlbumsResp = match self.fetch_json(&url, "artist albums").await {
            Some(d) => d,
            None => return (Vec::new(), false),
        };
        let body = parsed.response.unwrap_or(ArtistAlbumsBody {
            albums: None,
            next_page: None,
        });
        let has_more = body.next_page.is_some();
        let out = body
            .albums
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| {
                let id = a.id?;
                let name = a
                    .name
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())?;
                let year = a
                    .release_date_components
                    .and_then(|d| d.year)
                    .and_then(|y| i16::try_from(y).ok());
                Some(GeniusAlbumRef {
                    genius_album_id: id,
                    name,
                    year,
                    cover_url: a.cover_art_url.filter(|s| !s.is_empty()),
                })
            })
            .collect();
        (out, has_more)
    }

    pub async fn list_album_tracks(
        &self,
        genius_album_id: i64,
        page: u32,
        per_page: u32,
    ) -> (Vec<GeniusAlbumTrack>, bool) {
        let per = per_page.clamp(1, 50);
        let pg = page.max(1);
        let url = self.web_api(&format!(
            "/albums/{genius_album_id}/tracks?per_page={per}&page={pg}"
        ));
        let parsed: AlbumTracksResp = match self.fetch_json(&url, "album tracks").await {
            Some(d) => d,
            None => return (Vec::new(), false),
        };
        let body = parsed.response.unwrap_or(AlbumTracksBody {
            tracks: None,
            next_page: None,
        });
        let has_more = body.next_page.is_some();
        let tracks = body
            .tracks
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| {
                let song = t.song?;
                let id = song.id?;
                let title = song
                    .title
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())?;
                let primary = song.primary_artist.as_ref().and_then(map_artist);
                let featured = song
                    .featured_artists
                    .as_deref()
                    .map(|arr| arr.iter().filter_map(map_artist).collect())
                    .unwrap_or_default();
                Some(GeniusAlbumTrack {
                    genius_song_id: id,
                    title,
                    position: t.number,
                    primary_artist: primary,
                    featured,
                })
            })
            .collect();
        (tracks, has_more)
    }

    pub async fn lookup_song(&self, genius_song_id: i64) -> Option<GeniusSongDetails> {
        let path = format!("/songs/{genius_song_id}");
        let url = if self.has_token() {
            self.api(&path)
        } else {
            self.web_api(&path)
        };
        let parsed: SongResp = self.fetch_json(&url, "song").await?;
        let song = parsed.response.and_then(|r| r.song)?;
        let song_year = song
            .release_date_components
            .and_then(|d| d.year)
            .and_then(|y| i16::try_from(y).ok());
        let album = song.album.and_then(|a| {
            let id = a.id?;
            let name = a
                .name
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())?;
            let year = a
                .release_date_components
                .and_then(|d| d.year)
                .and_then(|y| i16::try_from(y).ok());
            Some(GeniusAlbumRef {
                genius_album_id: id,
                name,
                year,
                cover_url: a.cover_art_url,
            })
        });
        Some(GeniusSongDetails {
            album,
            year: song_year,
        })
    }

    pub async fn lookup_artist(&self, genius_id: i64) -> Option<GeniusArtistDetails> {
        let path = format!("/artists/{genius_id}");
        let url = if self.has_token() {
            self.api(&path)
        } else {
            self.web_api(&path)
        };
        let parsed: ArtistResp = self.fetch_json(&url, "artist").await?;
        let a = parsed.response.and_then(|r| r.artist)?;
        Some(GeniusArtistDetails {
            avatar_url: a.image_url,
            instagram: a.instagram_name.filter(|s| !s.is_empty()),
            twitter: a.twitter_name.filter(|s| !s.is_empty()),
            facebook: a.facebook_name.filter(|s| !s.is_empty()),
            url: a.url,
        })
    }

    pub async fn search_song_meta(&self, q: &str, limit: usize) -> Vec<GeniusSongMeta> {
        if self.has_token() {
            let api_hits = self.api_search(q, limit).await;
            if !api_hits.is_empty() {
                return api_hits;
            }
        }
        let Some(data) = self.fetch_web_search(q).await else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let sections = data.response.as_ref().and_then(|r| r.sections.as_ref());
        if let Some(secs) = sections {
            for section in secs {
                if section.type_ != "song" {
                    continue;
                }
                let Some(hits) = &section.hits else {
                    continue;
                };
                for hit in hits.iter().take(limit) {
                    let Some(result) = &hit.result else {
                        continue;
                    };
                    let Some(title) = result.title.clone() else {
                        continue;
                    };
                    let primary = result.primary_artist.as_ref().and_then(map_artist);
                    let featured = result
                        .featured_artists
                        .as_deref()
                        .map(|arr| arr.iter().filter_map(map_artist).collect())
                        .unwrap_or_default();
                    out.push(GeniusSongMeta {
                        genius_song_id: result.id,
                        title,
                        primary_artist: primary,
                        featured,
                    });
                }
            }
        }
        out
    }

    async fn api_search(&self, q: &str, limit: usize) -> Vec<GeniusSongMeta> {
        let url = self.api(&format!("/search?q={}", urlencoding::encode(q)));
        let parsed: ApiSearchResp = match self.fetch_json(&url, "api-search").await {
            Some(d) => d,
            None => return Vec::new(),
        };
        let hits = parsed.response.and_then(|r| r.hits).unwrap_or_default();
        hits.into_iter()
            .take(limit)
            .filter_map(|h| {
                let result = h.result?;
                let title = result.title?;
                let primary = result.primary_artist.as_ref().and_then(map_artist);
                let featured = result
                    .featured_artists
                    .as_deref()
                    .map(|arr| arr.iter().filter_map(map_artist).collect())
                    .unwrap_or_default();
                Some(GeniusSongMeta {
                    genius_song_id: result.id,
                    title,
                    primary_artist: primary,
                    featured,
                })
            })
            .collect()
    }

    async fn fetch_web_search(&self, q: &str) -> Option<SearchResp> {
        let url = self.web_api(&format!("/search/multi?q={}", urlencoding::encode(q)));
        self.fetch_json(&url, "search").await
    }

    pub async fn search_by_query(&self, q: &str, limit: usize) -> Vec<GeniusCandidate> {
        let hits = self.collect_lyric_hits(q, limit).await;
        let mut out = Vec::new();
        for hit in hits.into_iter().take(limit) {
            let html = match self.fetch_html(&hit.url).await {
                Some(t) => t,
                None => continue,
            };
            if let Some(plain) = parse_lyrics_html(&html) {
                out.push(GeniusCandidate {
                    plain_text: plain,
                    artist_guess: hit.artist,
                    title_guess: hit.title,
                });
            }
        }
        out
    }

    /// Собирает список Genius-кандидатов для скрейпа лирики:
    /// сначала через api.genius.com (direct → fallback) если есть токен,
    /// затем web /api/search/multi (proxy_first) как добор/фолбэк.
    async fn collect_lyric_hits(&self, q: &str, limit: usize) -> Vec<LyricHit> {
        let mut out: Vec<LyricHit> = Vec::new();
        let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();

        if self.has_token() {
            for h in self.api_search_hits(q, limit).await {
                if seen_urls.insert(h.url.clone()) {
                    out.push(h);
                }
            }
            if out.len() >= limit {
                return out;
            }
        }

        if let Some(data) = self.fetch_web_search(q).await {
            let sections = data.response.as_ref().and_then(|r| r.sections.as_ref());
            if let Some(secs) = sections {
                for section in secs {
                    if section.type_ != "song" {
                        continue;
                    }
                    let Some(hits) = &section.hits else { continue };
                    for hit in hits {
                        let Some(result) = &hit.result else { continue };
                        let Some(url) = &result.url else { continue };
                        if !seen_urls.insert(url.clone()) {
                            continue;
                        }
                        out.push(LyricHit {
                            url: url.clone(),
                            artist: result.primary_artist.as_ref().and_then(|a| a.name.clone()),
                            title: result.title.clone(),
                        });
                        if out.len() >= limit {
                            return out;
                        }
                    }
                }
            }
        }

        out
    }

    async fn api_search_hits(&self, q: &str, limit: usize) -> Vec<LyricHit> {
        let url = self.api(&format!("/search?q={}", urlencoding::encode(q)));
        let parsed: ApiSearchResp = match self.fetch_json(&url, "api-search-lyric").await {
            Some(d) => d,
            None => return Vec::new(),
        };
        let hits = parsed.response.and_then(|r| r.hits).unwrap_or_default();
        hits.into_iter()
            .take(limit)
            .filter_map(|h| {
                let result = h.result?;
                let url = result.url?;
                Some(LyricHit {
                    url,
                    artist: result.primary_artist.as_ref().and_then(|a| a.name.clone()),
                    title: result.title,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct LyricHit {
    url: String,
    artist: Option<String>,
    title: Option<String>,
}

fn map_artist(a: &PrimaryArtist) -> Option<GeniusArtistRef> {
    let name = a.name.as_ref()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(GeniusArtistRef {
        genius_artist_id: a.id,
        name: name.to_string(),
    })
}

fn parse_lyrics_html(html: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    while let Some(m) = RE_OPEN.find_at(html, cursor) {
        let start = m.end();
        if let Some(inner) = extract_balanced_div_content(html, start) {
            parts.push(inner);
        }
        cursor = m.end();
        if cursor >= html.len() {
            break;
        }
    }
    if parts.is_empty() {
        return None;
    }

    let mut text = parts.join("\n");
    text = RE_BR.replace_all(&text, "\n").into_owned();
    text = RE_TAGS.replace_all(&text, "").into_owned();
    text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"");

    text = RE_LEAD_CONTRIB.replace(&text, "").into_owned();
    text = RE_LEAD_LYRICS.replace(&text, "").into_owned();
    text = RE_LEAD_TEXT_PESN.replace(&text, "").into_owned();
    let trimmed = text.trim().to_string();
    if trimmed.len() > 20 {
        Some(trimmed)
    } else {
        None
    }
}

fn extract_balanced_div_content(html: &str, start_pos: usize) -> Option<String> {
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut depth = 1i32;
    let mut pos = start_pos;
    while pos < len && depth > 0 {
        let next_open = find_subseq(bytes, pos, b"<div");
        let next_close = find_subseq(bytes, pos, b"</div");
        let nc = match next_close {
            Some(p) => p,
            None => return None,
        };
        match next_open {
            Some(no) if no < nc => {
                let after_idx = no + 4;
                let after = if after_idx < len { bytes[after_idx] } else { 0 };
                if matches!(after, b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') {
                    depth += 1;
                }
                pos = no + 4;
            }
            _ => {
                depth -= 1;
                if depth == 0 {
                    return Some(html[start_pos..nc].to_string());
                }
                pos = nc + 5;
            }
        }
    }
    None
}

fn find_subseq(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    let n = needle.len();
    let mut i = from;
    while i + n <= haystack.len() {
        if &haystack[i..i + n] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_client() -> Arc<GeniusService> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("scd-test/0.1")
            .build()
            .unwrap();
        let fetcher = ExternalFetcher::new(http, String::new(), None);
        GeniusService::new(
            fetcher,
            GeniusCfg {
                access_token: String::new(),
            },
        )
    }

    #[tokio::test]
    #[ignore]
    async fn live_search_psychosis_x_ray() {
        let svc = build_client();
        let candidates = svc.search_song_meta("Psychosis x-ray", 5).await;
        assert!(!candidates.is_empty(), "Genius returned no candidates");
        let psychosis = candidates
            .iter()
            .find(|c| {
                c.primary_artist
                    .as_ref()
                    .map(|a| a.name.to_lowercase() == "psychosis")
                    .unwrap_or(false)
            })
            .expect("expected Psychosis as primary artist in results");
        assert_eq!(psychosis.title.to_lowercase(), "x-ray");
    }

    #[tokio::test]
    #[ignore]
    async fn live_list_psychosis_albums() {
        let svc = build_client();
        let (albums, _has_more) = svc.list_artist_albums(3401261, 1, 20).await;
        assert!(
            albums.len() >= 5,
            "expected several albums, got {}",
            albums.len()
        );
        let names: Vec<String> = albums.iter().map(|a| a.name.to_lowercase()).collect();
        assert!(
            names.iter().any(|n| n.contains("euphoria")),
            "euphoria not in {:?}",
            names
        );
    }

    #[tokio::test]
    #[ignore]
    async fn live_album_tracks_euphoria() {
        let svc = build_client();
        let (tracks, _) = svc.list_album_tracks(1222807, 1, 50).await;
        assert!(tracks.len() >= 5);
        assert!(tracks.iter().all(|t| t.genius_song_id > 0));
    }

    #[tokio::test]
    #[ignore]
    async fn live_search_eminem_lose_yourself() {
        let svc = build_client();
        let candidates = svc.search_song_meta("Eminem Lose Yourself", 5).await;
        assert!(!candidates.is_empty());
        assert!(candidates.iter().any(|c| c
            .primary_artist
            .as_ref()
            .map(|a| a.name.to_lowercase() == "eminem")
            .unwrap_or(false)));
    }
}
