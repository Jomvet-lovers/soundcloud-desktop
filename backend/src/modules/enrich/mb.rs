use std::sync::Arc;
use std::time::Duration;

use reqwest::header::HeaderMap;
use serde::Deserialize;
use tracing::debug;

use crate::common::external_fetch::ExternalFetcher;
use crate::common::throttle::Throttle;
use crate::error::AppResult;

const MB_BASE: &str = "https://musicbrainz.org/ws/2";

#[derive(Debug, Clone)]
pub struct MbArtist {
    pub mb_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct MbRelease {
    pub mb_id: String,
    pub title: String,
    pub year: Option<i16>,
    pub release_type: Option<String>,
    pub primary_artist: Option<MbArtist>,
}

#[derive(Debug, Clone)]
pub struct MbRecording {
    pub primary_artist: Option<MbArtist>,
    pub featured: Vec<MbArtist>,
    pub release: Option<MbRelease>,
    pub score: u32,
}

#[derive(Debug, Clone)]
pub struct MbArtistDetails {
    /// Имя СУЩНОСТИ (не кредит-алиас с релиза).
    pub name: Option<String>,
    pub country: Option<String>,
    pub disambiguation: Option<String>,
    pub urls: Vec<MbArtistUrl>,
}

#[derive(Debug, Clone)]
pub struct MbArtistUrl {
    pub kind: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct MbRecordingBrief {
    pub mb_id: String,
    pub title: String,
    pub length_ms: Option<i32>,
    pub first_release_year: Option<i16>,
    pub isrc: Option<String>,
    pub primary_artist: Option<MbArtist>,
    pub featured: Vec<MbArtist>,
    pub release: Option<MbReleaseBrief>,
}

#[derive(Debug, Clone)]
pub struct MbReleaseBrief {
    pub mb_id: String,
    pub title: String,
    pub year: Option<i16>,
    pub release_type: Option<String>,
}

pub struct MbClient {
    fetcher: Arc<ExternalFetcher>,
    user_agent: String,
    throttle: Arc<Throttle>,
}

impl MbClient {
    pub fn new(fetcher: Arc<ExternalFetcher>, user_agent: String, rate_limit_ms: u64) -> Arc<Self> {
        Arc::new(Self {
            fetcher,
            user_agent,
            throttle: Throttle::new(Duration::from_millis(rate_limit_ms.max(1100))),
        })
    }

    async fn fetch<T: for<'de> Deserialize<'de>>(&self, url: &str) -> AppResult<Option<T>> {
        let mut headers = HeaderMap::new();
        if let Ok(v) = self.user_agent.parse() {
            headers.insert("User-Agent", v);
        }
        if let Ok(v) = "application/json".parse() {
            headers.insert("Accept", v);
        }
        let bytes = match self.fetcher.get_api(url, headers, &self.throttle).await {
            Ok(b) => b,
            Err(crate::error::AppError::ScApi { status: 404, .. }) => return Ok(None),
            Err(e) => {
                debug!(url, error = %e, "MB fetch failed");
                return Ok(None);
            }
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(d) => Ok(Some(d)),
            Err(e) => {
                debug!(url, error = %e, "MB parse failed");
                Ok(None)
            }
        }
    }

    pub async fn lookup_by_isrc(&self, isrc: &str) -> AppResult<Option<MbRecording>> {
        let url = format!(
            "{MB_BASE}/isrc/{}?fmt=json&inc=artist-credits+releases+release-groups",
            urlencoding::encode(isrc)
        );
        let body: Option<IsrcResponse> = self.fetch(&url).await?;
        let Some(body) = body else {
            return Ok(None);
        };
        Ok(body
            .recordings
            .into_iter()
            .next()
            .map(|r| recording_from_payload(r, 100)))
    }

    pub async fn lookup_artist(&self, mb_id: &str) -> AppResult<Option<MbArtistDetails>> {
        let url = format!(
            "{MB_BASE}/artist/{}?inc=url-rels&fmt=json",
            urlencoding::encode(mb_id)
        );
        let body: Option<ArtistPayload> = self.fetch(&url).await?;
        Ok(body.map(|p| MbArtistDetails {
            name: p
                .name
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty()),
            country: p.country,
            disambiguation: p.disambiguation.filter(|s| !s.is_empty()),
            urls: p
                .relations
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| {
                    let kind = r.type_field?;
                    let resource = r.url.and_then(|u| u.resource)?;
                    if resource.is_empty() {
                        None
                    } else {
                        Some(MbArtistUrl {
                            kind,
                            url: resource,
                        })
                    }
                })
                .collect(),
        }))
    }

    pub async fn browse_recordings_by_artist(
        &self,
        mb_id: &str,
        offset: u32,
        limit: u32,
    ) -> AppResult<Vec<MbRecordingBrief>> {
        let limit = limit.clamp(1, 100);
        let url = format!(
            "{MB_BASE}/recording?artist={}&inc=artist-credits+isrcs+releases+release-groups&fmt=json&limit={limit}&offset={offset}",
            urlencoding::encode(mb_id)
        );
        let body: Option<BrowseResponse> = self.fetch(&url).await?;
        let Some(body) = body else {
            return Ok(Vec::new());
        };
        Ok(body
            .recordings
            .into_iter()
            .map(|r| {
                let credits: Vec<RawCredit> = r.artist_credit.unwrap_or_default();
                let mut artists: Vec<MbArtist> = credits
                    .into_iter()
                    .filter_map(|c| {
                        // Имя СУЩНОСТИ, не кредит-алиас: на релизе артист бывает
                        // подписан сценическим сокращением ("SID" у SIDODJI
                        // DUBOSHIT) — алиас минтит артиста-двойника.
                        c.artist.map(|a| MbArtist {
                            mb_id: a.id,
                            name: a.name,
                        })
                    })
                    .collect();
                let primary = if artists.is_empty() {
                    None
                } else {
                    Some(artists.remove(0))
                };
                let isrc = r
                    .isrcs
                    .unwrap_or_default()
                    .into_iter()
                    .find(|s| !s.is_empty());
                let year = r
                    .first_release_date
                    .as_deref()
                    .and_then(|s| s.split('-').next())
                    .and_then(|y| y.parse::<i16>().ok());
                let release = pick_best_release(r.releases.unwrap_or_default()).map(|rel| {
                    let rel_year = rel
                        .date
                        .as_deref()
                        .and_then(|s| s.split('-').next())
                        .and_then(|y| y.parse::<i16>().ok());
                    MbReleaseBrief {
                        mb_id: rel.id,
                        title: rel.title,
                        year: rel_year,
                        release_type: rel.release_group.and_then(|rg| rg.primary_type),
                    }
                });
                MbRecordingBrief {
                    mb_id: r.id,
                    title: r.title,
                    length_ms: r.length,
                    first_release_year: year,
                    isrc,
                    primary_artist: primary,
                    featured: artists,
                    release,
                }
            })
            .collect())
    }

    pub async fn search_recording(
        &self,
        artist: &str,
        title: &str,
        duration_ms: Option<i32>,
    ) -> AppResult<Option<MbRecording>> {
        let mut q = format!(
            "artist:\"{}\" AND recording:\"{}\"",
            mb_escape(artist),
            mb_escape(title)
        );
        if let Some(ms) = duration_ms {
            let secs = ms / 1000;
            q.push_str(&format!(
                " AND dur:[{} TO {}]",
                (secs - 5).max(0) * 1000,
                (secs + 5) * 1000
            ));
        }
        let url = format!(
            "{MB_BASE}/recording/?query={}&fmt=json&limit=5&inc=artist-credits+releases",
            urlencoding::encode(&q)
        );
        let body: Option<SearchResponse> = self.fetch(&url).await?;
        let Some(body) = body else {
            return Ok(None);
        };
        let best = body
            .recordings
            .into_iter()
            .filter(|r| r.score.unwrap_or(0) >= 80)
            .max_by_key(|r| r.score.unwrap_or(0));
        Ok(best.map(|r| {
            let score = r.score.unwrap_or(0);
            recording_from_payload(r, score)
        }))
    }
}

fn pick_best_release(releases: Vec<RawRelease>) -> Option<RawRelease> {
    let mut scored: Vec<(i32, RawRelease)> = releases
        .into_iter()
        .map(|r| {
            let t = r
                .release_group
                .as_ref()
                .and_then(|g| g.primary_type.as_deref());
            let s = match t {
                Some("Album") => 100,
                Some("Soundtrack") => 80,
                Some("EP") => 70,
                Some("Single") => 60,
                Some("Compilation") => -10,
                Some("Broadcast") => 0,
                Some("Other") => 5,
                None => 5,
                Some(_) => 10,
            };
            (s, r)
        })
        .collect();
    scored.sort_by_key(|(s, _)| -s);
    scored.into_iter().next().map(|(_, r)| r)
}

fn mb_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '"' | '\\'
                | '+'
                | '-'
                | '!'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '^'
                | '~'
                | '*'
                | '?'
                | ':'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn recording_from_payload(r: RecordingPayload, score: u32) -> MbRecording {
    let credits: Vec<RawCredit> = r.artist_credit.unwrap_or_default();
    let mut artists: Vec<MbArtist> = credits
        .iter()
        .filter_map(|c| {
            c.artist.as_ref().map(|a| MbArtist {
                mb_id: a.id.clone(),
                name: a.name.clone(),
            })
        })
        .collect();
    let primary = if artists.is_empty() {
        None
    } else {
        Some(artists.remove(0))
    };
    let release = pick_best_release(r.releases.unwrap_or_default()).map(|rel| {
        let year = rel
            .date
            .as_deref()
            .and_then(|s| s.split('-').next())
            .and_then(|y| y.parse::<i16>().ok());
        let release_credits: Vec<RawCredit> = rel.artist_credit.unwrap_or_default();
        let release_primary = release_credits.into_iter().next().and_then(|c| {
            c.artist.map(|a| MbArtist {
                mb_id: a.id,
                name: a.name,
            })
        });
        MbRelease {
            mb_id: rel.id,
            title: rel.title,
            year,
            release_type: rel.release_group.and_then(|rg| rg.primary_type),
            primary_artist: release_primary,
        }
    });
    MbRecording {
        primary_artist: primary,
        featured: artists,
        release,
        score,
    }
}

#[derive(Debug, Deserialize)]
struct IsrcResponse {
    #[serde(default)]
    recordings: Vec<RecordingPayload>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    recordings: Vec<RecordingPayload>,
}

#[derive(Debug, Deserialize)]
struct RecordingPayload {
    #[serde(default)]
    score: Option<u32>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Option<Vec<RawCredit>>,
    #[serde(default)]
    releases: Option<Vec<RawRelease>>,
}

#[derive(Debug, Deserialize)]
struct BrowseResponse {
    #[serde(default)]
    recordings: Vec<BrowseRecording>,
}

#[derive(Debug, Deserialize)]
struct BrowseRecording {
    id: String,
    title: String,
    #[serde(default)]
    length: Option<i32>,
    #[serde(rename = "first-release-date", default)]
    first_release_date: Option<String>,
    #[serde(default)]
    isrcs: Option<Vec<String>>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Option<Vec<RawCredit>>,
    #[serde(default)]
    releases: Option<Vec<RawRelease>>,
}

#[derive(Debug, Deserialize)]
struct RawCredit {
    #[serde(default)]
    artist: Option<RawArtist>,
}

#[derive(Debug, Deserialize)]
struct RawArtist {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawRelease {
    id: String,
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Option<Vec<RawCredit>>,
    #[serde(rename = "release-group", default)]
    release_group: Option<RawReleaseGroup>,
}

#[derive(Debug, Deserialize)]
struct RawReleaseGroup {
    #[serde(rename = "primary-type", default)]
    primary_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtistPayload {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(default)]
    relations: Option<Vec<ArtistRelation>>,
}

#[derive(Debug, Deserialize)]
struct ArtistRelation {
    #[serde(rename = "type", default)]
    type_field: Option<String>,
    #[serde(default)]
    url: Option<ArtistRelationUrl>,
}

#[derive(Debug, Deserialize)]
struct ArtistRelationUrl {
    #[serde(default)]
    resource: Option<String>,
}
