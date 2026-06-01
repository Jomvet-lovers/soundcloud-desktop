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

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::int8 FROM wanted_tracks WHERE ($1::text IS NULL OR status = $1)",
    )
        .bind(&status)
        .fetch_one(&state.pg)
        .await?;

    let items = sqlx::query_as::<_, WantedTrackRow>(
        "SELECT w.id, w.title, w.status, w.source, w.external_id, w.isrc, w.release_year, \
                w.primary_artist_id, a.name AS primary_artist_name, w.track_id, \
                w.resolve_attempts, w.resolve_error, w.discovered_at, w.updated_at \
         FROM wanted_tracks w \
         LEFT JOIN artists a ON a.id = w.primary_artist_id \
         WHERE ($1::text IS NULL OR w.status = $1) \
         ORDER BY w.discovered_at DESC \
         LIMIT $2 OFFSET $3",
    )
        .bind(&status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pg)
        .await?;

    let by_status: Vec<StatusCount> = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*)::int8 FROM wanted_tracks GROUP BY status ORDER BY COUNT(*) DESC",
    )
        .fetch_all(&state.pg)
        .await?
        .into_iter()
        .map(|(status, count)| StatusCount { status, count })
        .collect();

    Ok(Json(WantedTracksPage { items, total, page, limit, by_status }))
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

    let row: Option<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT status, track_id FROM wanted_tracks WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pg)
            .await?;
    match row {
        None => Err(AppError::not_found("wanted track not found")),
        Some(_) if !linked => Err(AppError::bad_request(
            "no tracks row matches sc_track_id; wanted track left unlinked",
        )),
        Some((status, track_id)) => Ok(Json(serde_json::json!({
            "ok": true,
            "linked": track_id.is_some(),
            "status": status,
            "track_id": track_id,
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
    let res = sqlx::query("UPDATE wanted_tracks SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status)
        .bind(id)
        .execute(&state.pg)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::not_found("wanted track not found"));
    }
    Ok(Json(serde_json::json!({ "ok": true, "status": status })))
}
