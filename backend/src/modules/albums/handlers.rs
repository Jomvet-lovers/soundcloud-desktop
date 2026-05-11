use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::common::session::SessionCtx;
use crate::error::{AppError, AppResult};
use crate::modules::enrich::dto as enrich_dto;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/albums/{id}", get(detail))
}

#[derive(Debug, Serialize)]
struct AlbumDetailDto {
    id: Uuid,
    title: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_year: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_url: Option<String>,
    confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    primary_artist: Option<AlbumArtist>,
    artists: Vec<AlbumArtist>,
    tracks: Vec<Value>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct AlbumArtist {
    id: Uuid,
    name: String,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
}

async fn detail(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(id): Path<Uuid>,
) -> AppResult<Json<AlbumDetailDto>> {
    let row: Option<(String, String, Option<i16>, Option<String>, f32, Option<Uuid>)> =
        sqlx::query_as(
            "SELECT title, type, release_year, cover_url, confidence, primary_artist_id
             FROM albums WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&st.pg)
        .await?;
    let Some((title, kind, release_year, cover_url, confidence, primary_artist_id)) = row else {
        return Err(AppError::not_found("album not found"));
    };

    let artists: Vec<AlbumArtist> = sqlx::query_as(
        "SELECT a.id, a.name, aa.role, a.avatar_url
         FROM album_artists aa
         JOIN artists a ON a.id = aa.artist_id
         WHERE aa.album_id = $1
         ORDER BY CASE aa.role WHEN 'primary' THEN 0 WHEN 'featured' THEN 1 ELSE 2 END, a.name",
    )
    .bind(id)
    .fetch_all(&st.pg)
    .await?;

    let primary_artist = if let Some(pa_id) = primary_artist_id {
        let row: Option<AlbumArtist> = sqlx::query_as(
            "SELECT id, name, 'primary'::text AS role, avatar_url
             FROM artists WHERE id = $1 AND merged_into IS NULL",
        )
        .bind(pa_id)
        .fetch_optional(&st.pg)
        .await?;
        row
    } else {
        None
    };

    let track_rows: Vec<(Option<sqlx::types::Json<Value>>, Option<i16>)> = sqlx::query_as(
        "SELECT it.raw_sc_data, at.position
         FROM album_tracks at
         JOIN indexed_tracks it ON it.id = at.indexed_track_id
         WHERE at.album_id = $1 AND it.raw_sc_data IS NOT NULL
         ORDER BY COALESCE(at.position, 32767), it.created_at",
    )
    .bind(id)
    .fetch_all(&st.pg)
    .await?;
    let mut tracks: Vec<Value> = Vec::with_capacity(track_rows.len());
    for (raw, _pos) in track_rows {
        if let Some(j) = raw {
            tracks.push(j.0);
        }
    }
    enrich_dto::apply_to_tracks(&st.pg, &mut tracks).await?;

    let wanted_rows: Vec<(Uuid, String, Option<i32>, Option<i16>, Option<Uuid>, Option<String>, i16)> =
        sqlx::query_as(
            "SELECT wt.id, wt.title, wt.duration_ms, wt.release_year, wt.primary_artist_id, a.name, wta.position
             FROM wanted_track_albums wta
             JOIN wanted_tracks wt ON wt.id = wta.wanted_track_id
             LEFT JOIN artists a ON a.id = wt.primary_artist_id
             WHERE wta.album_id = $1
               AND wt.indexed_track_id IS NULL
               AND wt.status = 'wanted'
             ORDER BY wta.position, wt.title",
        )
        .bind(id)
        .fetch_all(&st.pg)
        .await?;
    for (wid, title, dur_ms, year, pa_id, artist_name, _pos) in wanted_rows {
        tracks.push(serde_json::json!({
            "urn": format!("wanted:tracks:{}", wid),
            "id": 0,
            "title": title,
            "duration": dur_ms.unwrap_or(0),
            "artwork_url": null,
            "user": {
                "id": 0,
                "urn": "",
                "username": artist_name.clone().unwrap_or_default(),
                "avatar_url": "",
                "permalink_url": "",
            },
            "enrichment": {
                "state": "done",
                "upload_kind": "unknown",
                "availability": "wanted",
                "primary_artist": pa_id.and_then(|aid| artist_name.as_ref().map(|n| serde_json::json!({
                    "id": aid,
                    "name": n,
                    "source": "genius_crawl",
                    "confidence": 1.0,
                    "verified": true,
                }))),
                "release_year": year,
            },
        }));
    }

    Ok(Json(AlbumDetailDto {
        id,
        title,
        kind,
        release_year,
        cover_url,
        confidence,
        primary_artist,
        artists,
        tracks,
    }))
}
