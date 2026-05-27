use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::cache::ListPageResult;
use crate::common::pagination::PaginationQuery;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search/db/tracks", get(tracks))
        .route("/search/db/playlists", get(playlists))
        .route("/search/db/users", get(users))
        .route("/search/db/artists", get(artists))
        .route("/search/db/albums", get(albums))
}

#[derive(Debug, Clone, Deserialize)]
struct CommonSearchQuery {
    #[serde(default)]
    q: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ScopedSearchQuery {
    #[serde(default)]
    q: Option<String>,
    /// Опциональный фильтр: ограничить выдачу контентом конкретного юзера
    /// (его tracks / playlists). Полезно для inline-поиска на UserPage.
    #[serde(default)]
    user_urn: Option<String>,
}

async fn tracks(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<ScopedSearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let query = q.q.unwrap_or_default();
    let user = q.user_urn.filter(|s| !s.is_empty());
    Ok(Json(
        st.search
            .tracks(&query, user.as_deref(), page, limit)
            .await?,
    ))
}

async fn playlists(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<ScopedSearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let query = q.q.unwrap_or_default();
    let user = q.user_urn.filter(|s| !s.is_empty());
    Ok(Json(
        st.search
            .playlists(&query, user.as_deref(), page, limit)
            .await?,
    ))
}

async fn users(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<CommonSearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let query = q.q.unwrap_or_default();
    Ok(Json(st.search.users(&query, page, limit).await?))
}

async fn artists(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<CommonSearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let query = q.q.unwrap_or_default();
    Ok(Json(st.search.artists(&query, page, limit).await?))
}

async fn albums(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<CommonSearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let query = q.q.unwrap_or_default();
    Ok(Json(st.search.albums(&query, page, limit).await?))
}
