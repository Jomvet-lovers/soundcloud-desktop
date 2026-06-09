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

#[derive(Debug, Serialize)]
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
    let row = sqlx::query_file!("queries/albums/handlers/detail_album.sql", id)
        .fetch_optional(&st.pg)
        .await?;
    let Some(row) = row else {
        return Err(AppError::not_found("album not found"));
    };
    let (title, kind, release_year, cover_url, confidence, primary_artist_id) = (
        row.title,
        row.r#type,
        row.release_year,
        row.cover_url,
        row.confidence,
        row.primary_artist_id,
    );

    let artists: Vec<AlbumArtist> =
        sqlx::query_file!("queries/albums/handlers/detail_artists.sql", id)
            .fetch_all(&st.pg)
            .await?
            .into_iter()
            .map(|r| AlbumArtist {
                id: r.id,
                name: r.name,
                role: r.role,
                avatar_url: r.avatar_url,
            })
            .collect();

    let primary_artist = if let Some(pa_id) = primary_artist_id {
        sqlx::query_file!("queries/albums/handlers/detail_primary_artist.sql", pa_id)
            .fetch_optional(&st.pg)
            .await?
            .map(|r| AlbumArtist {
                id: r.id,
                name: r.name,
                role: r.role,
                avatar_url: r.avatar_url,
            })
    } else {
        None
    };

    let track_ids: Vec<String> =
        sqlx::query_file_scalar!("queries/albums/handlers/detail_track_ids.sql", id)
            .fetch_all(&st.pg)
            .await?;
    let mut tracks: Vec<Value> = crate::modules::tracks::project_many_public(&st.pg, &track_ids)
        .await?
        .into_iter()
        .flatten()
        .collect();
    enrich_dto::apply_to_tracks(&st.pg, &mut tracks).await?;

    let wanted_rows = sqlx::query_file!("queries/albums/handlers/detail_wanted_tracks.sql", id)
        .fetch_all(&st.pg)
        .await?;
    for r in wanted_rows {
        let (wid, title, dur_ms, year, pa_id, artist_name) = (
            r.id,
            r.title,
            r.duration_ms,
            r.release_year,
            r.primary_artist_id,
            r.name,
        );
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
