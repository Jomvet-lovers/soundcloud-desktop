use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde::Deserialize;
use tracing::debug;

const GENIUS_SEARCH: &str = "https://genius.com/api/search/multi";
const TIMEOUT: Duration = Duration::from_secs(15);
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

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
    url: Option<String>,
    title: Option<String>,
    primary_artist: Option<PrimaryArtist>,
}
#[derive(Debug, Deserialize)]
struct PrimaryArtist {
    name: Option<String>,
}

pub struct GeniusService {
    http: Client,
}

impl GeniusService {
    pub fn new(http: Client) -> Arc<Self> {
        Arc::new(Self { http })
    }

    pub async fn search_by_query(&self, q: &str, limit: usize) -> Vec<GeniusCandidate> {
        let search_url = format!("{GENIUS_SEARCH}?q={}", urlencoding::encode(q));
        let resp = match self
            .http
            .get(&search_url)
            .header("User-Agent", UA)
            .header("Accept", "application/json")
            .timeout(TIMEOUT)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                debug!(status = %r.status(), "Genius search non-2xx");
                return Vec::new();
            }
            Err(e) => {
                debug!(error = %e, "Genius search failed");
                return Vec::new();
            }
        };
        let data: SearchResp = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                debug!(error = %e, "Genius search parse failed");
                return Vec::new();
            }
        };
        let mut hits: Vec<&SearchHit> = Vec::new();
        let sections = data.response.as_ref().and_then(|r| r.sections.as_ref());
        if let Some(secs) = sections {
            for section in secs {
                if section.type_ == "song" {
                    if let Some(h) = &section.hits {
                        hits.extend(h.iter());
                    }
                }
            }
        }

        let mut out = Vec::new();
        for hit in hits.iter().take(limit) {
            let result = match &hit.result {
                Some(r) => r,
                None => continue,
            };
            let url = match &result.url {
                Some(u) => u.clone(),
                None => continue,
            };
            let html = match self
                .http
                .get(&url)
                .header("User-Agent", UA)
                .timeout(TIMEOUT)
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => match r.text().await {
                    Ok(t) => t,
                    Err(e) => {
                        debug!(error = %e, "Genius page text failed");
                        continue;
                    }
                },
                Ok(r) => {
                    debug!(status = %r.status(), "Genius page non-2xx");
                    continue;
                }
                Err(e) => {
                    debug!(error = %e, "Genius page fetch failed");
                    continue;
                }
            };
            if let Some(plain) = parse_lyrics_html(&html) {
                out.push(GeniusCandidate {
                    plain_text: plain,
                    artist_guess: result.primary_artist.as_ref().and_then(|a| a.name.clone()),
                    title_guess: result.title.clone(),
                });
            }
        }
        out
    }
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
