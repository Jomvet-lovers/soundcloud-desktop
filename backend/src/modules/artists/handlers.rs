use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::common::pagination::PaginationQuery;
use crate::common::session::SessionCtx;
use crate::error::{AppError, AppResult};
use crate::modules::enrich::dto as enrich_dto;
use crate::modules::enrich::normalize::normalize_name;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/artists/by-name/{normalized}", get(by_name))
        .route("/artists/{id}", get(detail))
        .route("/artists/{id}/tracks", get(tracks))
        .route("/artists/{id}/covers", get(covers))
        .route("/artists/{id}/albums", get(albums))
        .route("/artists/{id}/star", get(star))
}

#[derive(Debug, Serialize)]
struct ArtistDetailDto {
    id: Uuid,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    confidence: f32,
    socials: Vec<SocialDto>,
    sc_accounts: Vec<ScAccountDto>,
    track_count: i64,
    track_count_primary: i64,
    track_count_featured: i64,
    album_count: i64,
    popular_tracks: Vec<Value>,
    related_artists: Vec<RelatedArtistDto>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct SocialDto {
    kind: String,
    url: String,
    source: String,
    verified: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct ScAccountDto {
    sc_user_id: String,
    role: String,
    source: String,
    verified: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct RelatedArtistDto {
    id: Uuid,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar_url: Option<String>,
    weight: f32,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct AlbumListItem {
    id: Uuid,
    title: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "kind")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_year: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_url: Option<String>,
    role: String,
}

#[derive(Debug, Deserialize)]
struct TracksQuery {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    sort: Option<String>,
}

async fn detail(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ArtistDetailDto>> {
    let row = sqlx::query_file!("queries/artists/handlers/detail_artist.sql", id)
        .fetch_optional(&st.pg)
        .await?;
    let Some(row) = row else {
        return Err(AppError::not_found("artist not found"));
    };
    let (name, country, bio, avatar_url, confidence) = (
        row.name,
        row.country,
        row.bio,
        row.avatar_url,
        row.confidence,
    );

    let socials: Vec<SocialDto> =
        sqlx::query_file!("queries/artists/handlers/detail_socials.sql", id)
            .fetch_all(&st.pg)
            .await?
            .into_iter()
            .map(|r| SocialDto {
                kind: r.kind,
                url: r.url,
                source: r.source,
                verified: r.verified,
            })
            .collect();

    let sc_accounts: Vec<ScAccountDto> =
        sqlx::query_file!("queries/artists/handlers/detail_sc_accounts.sql", id)
            .fetch_all(&st.pg)
            .await?
            .into_iter()
            .map(|r| ScAccountDto {
                sc_user_id: r.sc_user_id,
                role: r.role,
                source: r.source,
                verified: r.verified,
            })
            .collect();

    let track_counts = sqlx::query_file!("queries/artists/handlers/detail_track_counts.sql", id)
        .fetch_one(&st.pg)
        .await?;
    let track_count_primary = track_counts.primary;
    let track_count_featured = track_counts.featured;
    let track_count = track_count_primary + track_count_featured;

    let album_count =
        sqlx::query_file_scalar!("queries/artists/handlers/detail_album_count.sql", id)
            .fetch_one(&st.pg)
            .await?;

    let mut popular_tracks = fetch_artist_tracks(&st.pg, id, "any", "popular", 1, 6).await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut popular_tracks).await?;

    let related_artists: Vec<RelatedArtistDto> =
        sqlx::query_file!("queries/artists/handlers/detail_related.sql", id)
            .fetch_all(&st.pg)
            .await?
            .into_iter()
            .map(|r| RelatedArtistDto {
                id: r.id,
                name: r.name,
                country: r.country,
                avatar_url: r.avatar_url,
                weight: r.weight,
            })
            .collect();

    Ok(Json(ArtistDetailDto {
        id,
        name,
        country,
        bio,
        avatar_url,
        confidence,
        socials,
        sc_accounts,
        track_count,
        track_count_primary,
        track_count_featured,
        album_count,
        popular_tracks,
        related_artists,
    }))
}

async fn tracks(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(id): Path<Uuid>,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<TracksQuery>,
) -> AppResult<Json<Value>> {
    let (page, limit) = p.resolved();
    let role = q.role.as_deref().unwrap_or("any");
    let sort = q.sort.as_deref().unwrap_or("popular");
    let mut items = fetch_artist_tracks(&st.pg, id, role, sort, page, limit).await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut items).await?;
    if page <= 0 && role != "featured" {
        let wanted = fetch_wanted_stubs(&st.pg, id, 200).await?;
        items.extend(wanted);
    }
    Ok(Json(serde_json::json!({
        "collection": items,
        "page": page,
        "page_size": limit,
    })))
}

/// Все треки где этот артист — `cover_of_artist_id`. То есть кавера
/// сторонних uploader'ов на оригинал этого артиста.
async fn covers(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(id): Path<Uuid>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<Value>> {
    let (page, limit) = p.resolved();
    let offset = page.max(0) * limit;
    let ids: Vec<String> = sqlx::query_file_scalar!(
        "queries/artists/handlers/covers_track_ids.sql",
        id,
        limit,
        offset
    )
    .fetch_all(&st.pg)
    .await?;
    let mut items: Vec<Value> = crate::modules::tracks::project_many_public(&st.pg, &ids)
        .await?
        .into_iter()
        .flatten()
        .collect();
    enrich_dto::apply_to_tracks(&st.pg, &mut items).await?;
    Ok(Json(serde_json::json!({
        "collection": items,
        "page": page,
        "page_size": limit,
    })))
}

async fn albums(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<AlbumListItem>>> {
    let rows: Vec<AlbumListItem> =
        sqlx::query_file!("queries/artists/handlers/albums_list.sql", id)
            .fetch_all(&st.pg)
            .await?
            .into_iter()
            .map(|r| AlbumListItem {
                id: r.id,
                title: r.title,
                kind: r.kind,
                release_year: r.release_year,
                cover_url: r.cover_url,
                role: r.role,
            })
            .collect();
    Ok(Json(rows))
}

#[derive(Debug, Serialize)]
struct ArtistStarResponse {
    premium: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    aura_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_sc_user_id: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ScAccountTrustRow {
    sc_user_id: String,
    role: String,
    source: String,
    verified: bool,
}

fn source_rank(source: &str) -> i32 {
    match source {
        "isrc" | "sc_verified" => 0,
        "mb" => 1,
        "genius" => 2,
        "spotify" => 3,
        "manual" => 4,
        "ai" | "ai_resolver" => 5,
        _ => 6,
    }
}

fn role_rank(role: &str) -> i32 {
    match role {
        "main" => 0,
        "alt" => 1,
        "label" => 2,
        _ => 3,
    }
}

async fn star(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(id): Path<Uuid>,
) -> AppResult<Json<ArtistStarResponse>> {
    let mut accounts: Vec<ScAccountTrustRow> =
        sqlx::query_file!("queries/artists/handlers/star_sc_accounts.sql", id)
            .fetch_all(&st.pg)
            .await?
            .into_iter()
            .map(|r| ScAccountTrustRow {
                sc_user_id: r.sc_user_id,
                role: r.role,
                source: r.source,
                verified: r.verified,
            })
            .collect();

    accounts.sort_by_key(|a| {
        (
            !a.verified,
            role_rank(&a.role),
            source_rank(&a.source),
            a.sc_user_id.clone(),
        )
    });

    for acc in accounts {
        let urn = format!("soundcloud:users:{}", acc.sc_user_id);
        if !st.subscriptions.is_premium(&urn).await? {
            continue;
        }
        let aura = st.auras.get(&urn).await?;
        return Ok(Json(ArtistStarResponse {
            premium: true,
            aura_id: aura.as_ref().map(|a| a.aura_id.clone()),
            custom_hex: aura.and_then(|a| a.custom_hex),
            source_sc_user_id: Some(acc.sc_user_id),
        }));
    }

    Ok(Json(ArtistStarResponse {
        premium: false,
        aura_id: None,
        custom_hex: None,
        source_sc_user_id: None,
    }))
}

async fn by_name(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(normalized): Path<String>,
) -> AppResult<Json<Value>> {
    let n = normalize_name(&normalized);
    if n.is_empty() {
        return Err(AppError::bad_request("empty name"));
    }
    let row = sqlx::query_file!("queries/artists/handlers/by_name.sql", &n)
        .fetch_optional(&st.pg)
        .await?;
    match row {
        Some(row) => Ok(Json(serde_json::json!({ "id": row.id, "name": row.name }))),
        None => Err(AppError::not_found("artist not found")),
    }
}

async fn fetch_wanted_stubs(pg: &PgPool, artist_id: Uuid, limit: i64) -> AppResult<Vec<Value>> {
    let rows = sqlx::query_file!(
        "queries/artists/handlers/wanted_stubs.sql",
        artist_id,
        limit
    )
    .fetch_all(pg)
    .await?;
    let out = rows
        .into_iter()
        .map(|r| {
            let (wid, title, dur_ms, year, isrc, artist_name) = (
                r.id,
                r.title,
                r.duration_ms,
                r.release_year,
                r.isrc,
                r.artist_name,
            );
            serde_json::json!({
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
                    "primary_artist": artist_name.as_ref().map(|n| serde_json::json!({
                        "id": artist_id,
                        "name": n,
                        "source": "mb_crawl",
                        "confidence": 1.0,
                        "verified": true,
                    })),
                    "release_year": year,
                    "isrc": isrc,
                },
            })
        })
        .collect();
    Ok(out)
}

async fn fetch_artist_tracks(
    pg: &PgPool,
    artist_id: Uuid,
    role: &str,
    sort: &str,
    page: i64,
    limit: i64,
) -> AppResult<Vec<Value>> {
    let offset = page.max(0) * limit;
    // static arm per (role, sort) — keeps dropped columns failing at compile
    let recent = sort == "recent";
    let ids: Vec<String> = match (role, recent) {
        ("primary", false) => sqlx::query_file!(
            "queries/artists/handlers/tracks_primary_popular.sql",
            artist_id,
            limit,
            offset
        )
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| r.sc_track_id)
        .collect(),
        ("primary", true) => sqlx::query_file!(
            "queries/artists/handlers/tracks_primary_recent.sql",
            artist_id,
            limit,
            offset
        )
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| r.sc_track_id)
        .collect(),
        ("featured", false) => sqlx::query_file!(
            "queries/artists/handlers/tracks_featured_popular.sql",
            artist_id,
            limit,
            offset
        )
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| r.sc_track_id)
        .collect(),
        ("featured", true) => sqlx::query_file!(
            "queries/artists/handlers/tracks_featured_recent.sql",
            artist_id,
            limit,
            offset
        )
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| r.sc_track_id)
        .collect(),
        (_, false) => sqlx::query_file!(
            "queries/artists/handlers/tracks_any_popular.sql",
            artist_id,
            limit,
            offset
        )
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| r.sc_track_id)
        .collect(),
        (_, true) => sqlx::query_file!(
            "queries/artists/handlers/tracks_any_recent.sql",
            artist_id,
            limit,
            offset
        )
        .fetch_all(pg)
        .await?
        .into_iter()
        .map(|r| r.sc_track_id)
        .collect(),
    };
    let projected = crate::modules::tracks::project_many_public(pg, &ids).await?;
    Ok(projected.into_iter().flatten().collect())
}
