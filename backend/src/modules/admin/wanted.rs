use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::common::admin::AdminAuth;
use crate::error::{AppError, AppResult};
use crate::modules::enrich::wanted_resolver::link_wanted_to_sc;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct WantedTrackRow {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub source: String,
    pub external_id: Option<String>,
    pub isrc: Option<String>,
    pub release_year: Option<i16>,
    pub primary_artist_id: Option<Uuid>,
    pub primary_artist_name: Option<String>,
    pub track_id: Option<Uuid>,
    pub resolve_attempts: i16,
    pub resolve_error: Option<String>,
    pub discovered_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct WantedTracksPage {
    pub items: Vec<WantedTrackRow>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub by_status: Vec<StatusCount>,
}

/// GET /admin/wanted-tracks?status=&page=&limit= — orphan tracks the pipeline
/// wants but hasn't linked to a real `tracks` row yet.
#[tracing::instrument(skip_all)]
pub async fn list(
    _: AdminAuth,
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<WantedTracksPage>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page - 1) * limit;
    let status = q.status.filter(|s| !s.is_empty());

    let total: i64 =
        sqlx::query_file_scalar!("queries/admin/wanted/count_total.sql", status.as_deref())
            .fetch_one(&state.pg)
            .await?;

    let items = sqlx::query_file!(
        "queries/admin/wanted/list_page.sql",
        status.as_deref(),
        limit,
        offset
    )
    .fetch_all(&state.pg)
    .await?
    .into_iter()
    .map(|r| WantedTrackRow {
        id: r.id,
        title: r.title,
        status: r.status,
        source: r.source,
        external_id: r.external_id,
        isrc: r.isrc,
        release_year: r.release_year,
        primary_artist_id: r.primary_artist_id,
        primary_artist_name: r.primary_artist_name,
        track_id: r.track_id,
        resolve_attempts: r.resolve_attempts,
        resolve_error: r.resolve_error,
        discovered_at: r.discovered_at,
        updated_at: r.updated_at,
    })
    .collect();

    let by_status: Vec<StatusCount> = sqlx::query_file!("queries/admin/wanted/count_by_status.sql")
        .fetch_all(&state.pg)
        .await?
        .into_iter()
        .map(|r| StatusCount {
            status: r.status,
            count: r.count,
        })
        .collect();

    Ok(Json(WantedTracksPage {
        items,
        total,
        page,
        limit,
        by_status,
    }))
}

#[derive(Deserialize)]
pub struct LinkBody {
    pub sc_track_id: String,
}

/// POST /admin/wanted-tracks/{id}/link — resolve a wanted track to a real
/// `tracks` row by its SoundCloud track id (delegates to the resolver's
/// `link_wanted_to_sc`, which also re-points albums and flips status to linked).
#[tracing::instrument(skip_all)]
pub async fn link(
    _: AdminAuth,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<LinkBody>,
) -> AppResult<Json<serde_json::Value>> {
    let sc = body.sc_track_id.trim();
    if sc.is_empty() {
        return Err(AppError::bad_request("sc_track_id is required"));
    }
    let linked = link_wanted_to_sc(&state.pg, id, sc).await?;

    let row = sqlx::query_file!("queries/admin/wanted/get_status_track.sql", id)
        .fetch_optional(&state.pg)
        .await?;
    match row {
        None => Err(AppError::not_found("wanted track not found")),
        Some(_) if !linked => Err(AppError::bad_request(
            "no tracks row matches sc_track_id; wanted track left unlinked",
        )),
        Some(r) => Ok(Json(serde_json::json!({
            "ok": true,
            "linked": r.track_id.is_some(),
            "status": r.status,
            "track_id": r.track_id,
        }))),
    }
}

#[derive(Deserialize)]
pub struct StatusBody {
    pub status: String,
}

const ALLOWED_STATUS: [&str; 4] = ["wanted", "linked", "unresolvable", "skipped"];

/// PATCH /admin/wanted-tracks/{id}/status — manual status override.
#[tracing::instrument(skip_all)]
pub async fn set_status(
    _: AdminAuth,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<StatusBody>,
) -> AppResult<Json<serde_json::Value>> {
    let status = body.status.trim();
    if !ALLOWED_STATUS.contains(&status) {
        return Err(AppError::bad_request(
            "status must be one of: wanted, linked, unresolvable, skipped",
        ));
    }
    let res = sqlx::query_file!("queries/admin/wanted/update_status.sql", status, id)
        .execute(&state.pg)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("wanted track not found"));
    }
    Ok(Json(serde_json::json!({ "ok": true, "status": status })))
}
