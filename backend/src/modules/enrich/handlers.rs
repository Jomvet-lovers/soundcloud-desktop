use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::common::admin::AdminAuth;
use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::{AppError, AppResult};
use crate::modules::enrich::sc_accounts::{self, AccountRole};
use crate::modules::enrich::service::EnrichStats;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/enrich/stats", get(get_stats))
        .route("/admin/enrich/retry", post(post_retry))
        .route(
            "/admin/artists/{artist_id}/sc-accounts",
            post(upsert_account),
        )
        .route(
            "/admin/artists/{artist_id}/sc-accounts/{sc_user_id}",
            delete(delete_account),
        )
        .route(
            "/admin/artists/{src_id}/merge-into/{dst_id}",
            post(merge_artists),
        )
        .route("/admin/artists/{artist_id}/retry-crawl", post(retry_crawl))
        .route("/admin/artists/{artist_id}/run-crawl", post(run_crawl_now))
}

async fn retry_crawl(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(artist_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let r = sqlx::query_file!("queries/enrich/handlers/retry_crawl_reset.sql", artist_id)
        .execute(&st.pg)
        .await?;
    Ok(Json(json!({ "reset": r.rows_affected() })))
}

async fn run_crawl_now(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(artist_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let crawl = st.artist_crawl.clone();
    let resolver = st.wanted_resolver.clone();
    tokio::spawn(async move {
        if let Err(e) = crawl.run_for_artist(artist_id).await {
            tracing::warn!(%artist_id, error = %e, "run_for_artist failed");
            return;
        }
        if let Err(e) = resolver.run_for_artist(artist_id, 500).await {
            tracing::warn!(%artist_id, error = %e, "wanted-resolver run_for_artist failed");
        }
    });
    Ok(Json(json!({ "ok": true, "spawned": true })))
}

async fn get_stats(_: AdminAuth, State(st): State<AppState>) -> AppResult<Json<EnrichStats>> {
    Ok(Json(st.enrich.stats().await?))
}

#[derive(Debug, Deserialize)]
struct RetryRequest {
    #[serde(default)]
    sc_track_id: Option<String>,
    #[serde(default)]
    all_failed: bool,
}

#[derive(Debug, Serialize)]
struct RetryResponse {
    reset: u64,
}

async fn post_retry(
    _: AdminAuth,
    State(st): State<AppState>,
    Json(req): Json<RetryRequest>,
) -> AppResult<Json<RetryResponse>> {
    let mut reset = 0u64;
    if let Some(raw) = req.sc_track_id.as_deref() {
        let sc = normalize_sc_track_id(raw)
            .ok_or_else(|| AppError::bad_request("invalid sc_track_id"))?;
        let r = sqlx::query_file!("queries/enrich/handlers/retry_by_sc_track_id.sql", &sc)
            .execute(&st.pg)
            .await?;
        reset += r.rows_affected();
    }
    if req.all_failed {
        let r = sqlx::query_file!("queries/enrich/handlers/retry_all_failed.sql")
            .execute(&st.pg)
            .await?;
        reset += r.rows_affected();
    }
    Ok(Json(RetryResponse { reset }))
}

#[derive(Debug, Deserialize)]
struct UpsertAccountRequest {
    sc_user_id: String,
    #[serde(default = "default_role")]
    role: String,
    #[serde(default)]
    notes: Option<String>,
}

fn default_role() -> String {
    "main".to_string()
}

async fn upsert_account(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(artist_id): Path<Uuid>,
    Json(req): Json<UpsertAccountRequest>,
) -> AppResult<Json<Value>> {
    let role = AccountRole::parse(&req.role)
        .ok_or_else(|| AppError::bad_request("role must be main|demo|alt"))?;
    let sc = req.sc_user_id.trim();
    if sc.is_empty() {
        return Err(AppError::bad_request("sc_user_id required"));
    }
    sc_accounts::upsert(&st.pg, artist_id, sc, role, "manual", true).await?;
    if let Some(notes) = req.notes.as_deref() {
        sqlx::query_file!(
            "queries/enrich/handlers/update_account_notes.sql",
            artist_id,
            sc,
            notes
        )
        .execute(&st.pg)
        .await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_account(
    _: AdminAuth,
    State(st): State<AppState>,
    Path((artist_id, sc_user_id)): Path<(Uuid, String)>,
) -> AppResult<Json<Value>> {
    let removed = sc_accounts::delete(&st.pg, artist_id, &sc_user_id).await?;
    Ok(Json(json!({ "ok": removed })))
}

#[derive(Debug, Serialize)]
struct MergeResponse {
    moved_track_artists: u64,
    moved_album_artists: u64,
    moved_sc_accounts: u64,
    coplay_dropped: u64,
}

async fn merge_artists(
    _: AdminAuth,
    State(st): State<AppState>,
    Path((src, dst)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<MergeResponse>> {
    if src == dst {
        return Err(AppError::bad_request("src and dst must differ"));
    }
    let mut tx = st.pg.begin().await?;

    sqlx::query_file!("queries/enrich/handlers/merge_mark_artist.sql", dst, src)
        .execute(&mut *tx)
        .await?;

    sqlx::query_file!(
        "queries/enrich/handlers/merge_copy_track_artists.sql",
        dst,
        src
    )
    .execute(&mut *tx)
    .await?;
    let track_artists_moved = sqlx::query_file!(
        "queries/enrich/handlers/merge_delete_track_artists.sql",
        src
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query_file!(
        "queries/enrich/handlers/merge_copy_album_artists.sql",
        dst,
        src
    )
    .execute(&mut *tx)
    .await?;
    let album_artists_moved = sqlx::query_file!(
        "queries/enrich/handlers/merge_delete_album_artists.sql",
        src
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    sqlx::query_file!(
        "queries/enrich/handlers/merge_update_albums_primary.sql",
        dst,
        src
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "queries/enrich/handlers/merge_update_tracks_primary.sql",
        dst,
        src
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "queries/enrich/handlers/merge_update_wanted_primary.sql",
        dst,
        src
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query_file!(
        "queries/enrich/handlers/merge_copy_sc_accounts.sql",
        dst,
        src
    )
    .execute(&mut *tx)
    .await?;
    let sc_accounts_moved =
        sqlx::query_file!("queries/enrich/handlers/merge_delete_sc_accounts.sql", src)
            .execute(&mut *tx)
            .await?
            .rows_affected();

    sqlx::query_file!(
        "queries/enrich/handlers/merge_copy_wanted_track_artists.sql",
        dst,
        src
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "queries/enrich/handlers/merge_delete_wanted_track_artists.sql",
        src
    )
    .execute(&mut *tx)
    .await?;

    let coplay_dropped = sqlx::query_file!("queries/enrich/handlers/merge_delete_coplay.sql", src)
        .execute(&mut *tx)
        .await?
        .rows_affected();

    tx.commit().await?;
    Ok(Json(MergeResponse {
        moved_track_artists: track_artists_moved,
        moved_album_artists: album_artists_moved,
        moved_sc_accounts: sc_accounts_moved,
        coplay_dropped,
    }))
}
