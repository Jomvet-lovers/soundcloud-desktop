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
    let row: Option<(String, Option<String>, Option<String>, Option<String>, f32)> = sqlx::query_as(
        "SELECT name, country, bio, avatar_url, confidence
         FROM artists WHERE id = $1 AND merged_into IS NULL",
    )
    .bind(id)
    .fetch_optional(&st.pg)
    .await?;
    let Some((name, country, bio, avatar_url, confidence)) = row else {
        return Err(AppError::not_found("artist not found"));
    };

    let socials: Vec<SocialDto> = sqlx::query_as(
        "SELECT kind, url, source, verified FROM artist_socials WHERE artist_id = $1
         ORDER BY kind, url",
    )
    .bind(id)
    .fetch_all(&st.pg)
    .await?;

    let sc_accounts: Vec<ScAccountDto> = sqlx::query_as(
        "SELECT sc_user_id, role, source, verified
         FROM artist_sc_accounts
         WHERE artist_id = $1 AND role IN ('main', 'demo')
         ORDER BY verified DESC,
                  CASE role   WHEN 'main' THEN 0 WHEN 'demo' THEN 1 ELSE 2 END,
                  CASE source WHEN 'manual' THEN 0
                              WHEN 'auto_match' THEN 1
                              WHEN 'mb_resolve' THEN 2
                              ELSE 3 END,
                  sc_user_id",
    )
    .bind(id)
    .fetch_all(&st.pg)
    .await?;

    let track_counts: (i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COUNT(*)::bigint FROM (
                 SELECT indexed_track_id::text AS k FROM track_artists
                  WHERE artist_id = $1 AND role = 'primary'
                 UNION
                 SELECT id::text AS k FROM wanted_tracks
                  WHERE primary_artist_id = $1 AND indexed_track_id IS NULL AND status = 'wanted'
             ) p),
            (SELECT COUNT(DISTINCT indexed_track_id)::bigint FROM track_artists
              WHERE artist_id = $1 AND role IN ('featured', 'remixer'))",
    )
    .bind(id)
    .fetch_one(&st.pg)
    .await?;
    let (track_count_primary, track_count_featured) = track_counts;
    let track_count = track_count_primary + track_count_featured;

    let album_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM (
             SELECT id FROM albums WHERE primary_artist_id = $1
             UNION
             SELECT album_id FROM album_artists WHERE artist_id = $1
             UNION
             SELECT wta.album_id FROM wanted_track_albums wta
                JOIN wanted_tracks wt ON wt.id = wta.wanted_track_id
                WHERE wt.primary_artist_id = $1
         ) a",
    )
    .bind(id)
    .fetch_one(&st.pg)
    .await?;

    let mut popular_tracks = fetch_artist_tracks(&st.pg, id, "any", "popular", 1, 6).await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut popular_tracks).await?;

    let related_artists: Vec<RelatedArtistDto> = sqlx::query_as(
        "SELECT a.id, a.name, a.country, a.avatar_url, ac.weight
         FROM artist_coplay ac
         JOIN artists a ON a.id = CASE WHEN ac.a_id = $1 THEN ac.b_id ELSE ac.a_id END
         WHERE (ac.a_id = $1 OR ac.b_id = $1) AND a.merged_into IS NULL
         ORDER BY ac.weight DESC
         LIMIT 12",
    )
    .bind(id)
    .fetch_all(&st.pg)
    .await?;

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
        album_count: album_count.0,
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

async fn albums(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<AlbumListItem>>> {
    let rows: Vec<AlbumListItem> = sqlx::query_as(
        "SELECT al.id, al.title, al.type AS kind, al.release_year, al.cover_url,
                CASE WHEN al.primary_artist_id = $1 THEN 'primary' ELSE COALESCE(aa.role, 'featured') END AS role
         FROM albums al
         LEFT JOIN album_artists aa ON aa.album_id = al.id AND aa.artist_id = $1
         WHERE al.primary_artist_id = $1
            OR aa.artist_id = $1
            OR al.id IN (
                SELECT wta.album_id FROM wanted_track_albums wta
                JOIN wanted_tracks wt ON wt.id = wta.wanted_track_id
                WHERE wt.primary_artist_id = $1
            )
         ORDER BY COALESCE(al.release_year, 0) DESC, al.title",
    )
    .bind(id)
    .fetch_all(&st.pg)
    .await?;
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
    let mut accounts: Vec<ScAccountTrustRow> = sqlx::query_as(
        "SELECT sc_user_id, role, source, verified
         FROM artist_sc_accounts
         WHERE artist_id = $1 AND role IN ('main', 'demo')",
    )
    .bind(id)
    .fetch_all(&st.pg)
    .await?;

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
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM artists WHERE normalized_name = $1 AND merged_into IS NULL LIMIT 1",
    )
    .bind(&n)
    .fetch_optional(&st.pg)
    .await?;
    match row {
        Some((id, name)) => Ok(Json(serde_json::json!({ "id": id, "name": name }))),
        None => Err(AppError::not_found("artist not found")),
    }
}

async fn fetch_wanted_stubs(
    pg: &PgPool,
    artist_id: Uuid,
    limit: i64,
) -> AppResult<Vec<Value>> {
    let rows: Vec<(Uuid, String, Option<i32>, Option<i16>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT wt.id, wt.title, wt.duration_ms, wt.release_year, wt.isrc, a.name
             FROM wanted_tracks wt
             LEFT JOIN artists a ON a.id = wt.primary_artist_id
             WHERE wt.primary_artist_id = $1
               AND wt.indexed_track_id IS NULL
               AND wt.status = 'wanted'
             ORDER BY wt.discovered_at DESC
             LIMIT $2",
        )
        .bind(artist_id)
        .bind(limit)
        .fetch_all(pg)
        .await?;
    let out = rows
        .into_iter()
        .map(|(wid, title, dur_ms, year, isrc, artist_name)| {
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
    let role_filter = match role {
        "primary" => "ta.role = 'primary'",
        "featured" => "ta.role IN ('featured', 'remixer')",
        _ => "TRUE",
    };
    let order_clause = match sort {
        "recent" => "it.release_date DESC NULLS LAST, it.release_year DESC NULLS LAST, it.created_at DESC, it.id DESC",
        _ => "COALESCE(c.play_count, 0) DESC, it.created_at DESC",
    };
    let sql = format!(
        "SELECT it.raw_sc_data
         FROM track_artists ta
         JOIN indexed_tracks it ON it.id = ta.indexed_track_id
         LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
         WHERE ta.artist_id = $1
           AND {role_filter}
           AND it.raw_sc_data IS NOT NULL
         ORDER BY {order_clause}
         LIMIT $2 OFFSET $3"
    );
    let rows: Vec<(Option<sqlx::types::Json<Value>>,)> = sqlx::query_as(&sql)
        .bind(artist_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pg)
        .await?;
    Ok(rows.into_iter().filter_map(|(raw,)| raw.map(|j| j.0)).collect())
}
