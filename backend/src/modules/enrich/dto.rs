use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct EnrichmentDto {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub upload_kind: String,
    pub availability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_artist: Option<ArtistDto>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub participants: Vec<ParticipantDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<AlbumDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_year: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtistDto {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sc_user_id: Option<String>,
    pub source: String,
    pub confidence: f32,
    pub verified: bool,
}

fn artist_verified(source: &str, sc_user_id: Option<&str>) -> bool {
    matches!(source, "isrc" | "mb" | "genius" | "spotify" | "sc_verified") || sc_user_id.is_some()
}

#[derive(Debug, Clone, Serialize)]
pub struct ParticipantDto {
    pub artist: ArtistDto,
    pub role: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlbumDto {
    pub id: Uuid,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_artist: Option<ArtistDto>,
}

pub async fn lookup(pg: &PgPool, urns: &[String]) -> AppResult<HashMap<String, EnrichmentDto>> {
    let sc_ids: Vec<String> = urns
        .iter()
        .filter_map(|u| normalize_sc_track_id(u))
        .collect();
    if sc_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_file!("queries/enrich/dto/lookup_tracks.sql", &sc_ids)
        .fetch_all(pg)
        .await?;

    if rows.is_empty() {
        return Ok(HashMap::new());
    }

    let track_ids: Vec<Uuid> = rows.iter().map(|r| r.track_id).collect();
    let participants = sqlx::query_file!("queries/enrich/dto/lookup_participants.sql", &track_ids)
        .fetch_all(pg)
        .await?;

    let mut by_track: HashMap<Uuid, Vec<ParticipantDto>> = HashMap::new();
    for p in participants {
        let verified = artist_verified(&p.artist_source, p.artist_sc_user_id.as_deref());
        by_track
            .entry(p.track_id)
            .or_default()
            .push(ParticipantDto {
                artist: ArtistDto {
                    id: p.artist_id,
                    name: p.artist_name,
                    avatar_url: p.artist_avatar_url,
                    sc_user_id: p.artist_sc_user_id,
                    source: p.artist_source,
                    confidence: p.artist_confidence,
                    verified,
                },
                role: p.role,
                confidence: p.ta_confidence,
            });
    }

    let mut out: HashMap<String, EnrichmentDto> = HashMap::with_capacity(rows.len());
    for r in rows {
        let primary_artist = match (r.pa_id, r.pa_name) {
            (Some(id), Some(name)) => {
                let source = r.pa_source.unwrap_or_else(|| "heuristic".to_string());
                let verified = artist_verified(&source, r.pa_sc_user_id.as_deref());
                Some(ArtistDto {
                    id,
                    name,
                    avatar_url: r.pa_avatar_url,
                    sc_user_id: r.pa_sc_user_id,
                    source,
                    confidence: r.pa_confidence.unwrap_or(0.0),
                    verified,
                })
            }
            _ => None,
        };
        let album = match (r.al_id, r.al_title, r.al_kind) {
            (Some(id), Some(title), Some(kind)) => Some(AlbumDto {
                id,
                title,
                year: r.al_release_year,
                cover_url: r.al_cover_url,
                kind,
                primary_artist: match (r.aa_id, r.aa_name) {
                    (Some(aid), Some(aname)) => {
                        let source = r.aa_source.unwrap_or_else(|| "heuristic".to_string());
                        let verified = artist_verified(&source, r.aa_sc_user_id.as_deref());
                        Some(ArtistDto {
                            id: aid,
                            name: aname,
                            avatar_url: r.aa_avatar_url,
                            sc_user_id: r.aa_sc_user_id,
                            source,
                            confidence: r.aa_confidence.unwrap_or(0.0),
                            verified,
                        })
                    }
                    _ => None,
                },
            }),
            _ => None,
        };
        let participants = by_track.remove(&r.track_id).unwrap_or_default();
        let (release_year, release_date, release_source) = if let Some(date) = r.it_release_date {
            (
                r.it_release_year
                    .or_else(|| date.format("%Y").to_string().parse::<i16>().ok()),
                Some(date.format("%Y-%m-%d").to_string()),
                Some("sc_upload".to_string()),
            )
        } else if let Some(y) = r.it_release_year {
            (Some(y), None, Some("sc_upload".to_string()))
        } else if let Some(y) = album.as_ref().and_then(|a| a.year) {
            (Some(y), None, Some("album".to_string()))
        } else {
            (None, None, None)
        };
        out.insert(
            r.sc_track_id,
            EnrichmentDto {
                state: r.enrich_state,
                source: r.enrich_source,
                confidence: r.enrich_confidence,
                upload_kind: r.upload_kind,
                availability: "indexed".to_string(),
                primary_artist,
                participants,
                album,
                release_year,
                release_date,
                release_source,
            },
        );
    }
    Ok(out)
}

fn parse_year(s: &str) -> Option<i16> {
    if s.len() < 4 {
        return None;
    }
    s[..4]
        .parse::<i16>()
        .ok()
        .filter(|y| (1900..=2100).contains(y))
}

fn parse_iso_date(s: &str) -> Option<String> {
    if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' {
        let head = &s[..10];
        if head.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        }) {
            return Some(head.to_string());
        }
    }
    None
}

fn extract_sc_release(track: &Value) -> (Option<i16>, Option<String>) {
    let candidates = ["release_date", "display_date", "created_at"];
    for key in candidates {
        if let Some(s) = track.get(key).and_then(|v| v.as_str()) {
            if let Some(date) = parse_iso_date(s) {
                let year = parse_year(&date);
                return (year, Some(date));
            }
            if let Some(year) = parse_year(s) {
                return (Some(year), None);
            }
        }
    }
    let release_year = track
        .get("release_year")
        .and_then(|v| v.as_i64())
        .filter(|y| (1900..=2100).contains(&(*y as i16)))
        .map(|y| y as i16);
    (release_year, None)
}

pub async fn apply_to_tracks(pg: &PgPool, tracks: &mut [Value]) -> AppResult<()> {
    if tracks.is_empty() {
        return Ok(());
    }
    let urns: Vec<String> = tracks
        .iter()
        .filter_map(|t| t.get("urn").and_then(|v| v.as_str()).map(String::from))
        .collect();
    if urns.is_empty() {
        return Ok(());
    }
    crate::modules::tracks::counters::sync(pg, tracks).await?;
    let map = lookup(pg, &urns).await?;
    if map.is_empty() {
        return Ok(());
    }
    for t in tracks.iter_mut() {
        let Some(urn) = t.get("urn").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(sc_id) = normalize_sc_track_id(urn) else {
            continue;
        };
        let Some(enrichment) = map.get(&sc_id) else {
            continue;
        };
        let mut filled = enrichment.clone();
        if filled.release_year.is_none() {
            let (year, date) = extract_sc_release(t);
            if year.is_some() || date.is_some() {
                filled.release_year = year;
                filled.release_date = date;
                filled.release_source = Some("sc_upload".to_string());
            }
        }
        if let Some(obj) = t.as_object_mut() {
            if let Ok(value) = serde_json::to_value(&filled) {
                obj.insert("enrichment".into(), value);
            }
        }
    }
    Ok(())
}

pub async fn apply_to_track(pg: &PgPool, track: &mut Value) -> AppResult<()> {
    apply_to_tracks(pg, std::slice::from_mut(track)).await
}
