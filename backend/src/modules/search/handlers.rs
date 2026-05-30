use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::cache::ListPageResult;
use crate::common::pagination::PaginationQuery;
use crate::common::query::parse_languages;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::search::vibe::{LyricsMode, LyricsSearchResponse, VibeResponse};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search/db/tracks", get(tracks))
        .route("/search/db/playlists", get(playlists))
        .route("/search/db/users", get(users))
        .route("/search/db/artists", get(artists))
        .route("/search/db/albums", get(albums))
        .route("/search/vibe", get(vibe))
        .route("/search/lyrics", get(lyrics))
}

#[derive(Debug, Deserialize)]
struct VibeQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
}

async fn vibe(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Query(q): Query<VibeQuery>,
) -> AppResult<Json<VibeResponse>> {
    let limit = q.limit.as_deref().and_then(|s| s.parse::<usize>().ok());
    let languages = parse_languages(q.languages.as_deref());
    Ok(Json(
        st.vibe
            .vibe(&q.q.unwrap_or_default(), limit, languages.as_deref())
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
struct LyricsQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    page: Option<String>,
    #[serde(default)]
    limit: Option<String>,
}

async fn lyrics(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Query(q): Query<LyricsQuery>,
) -> AppResult<Json<LyricsSearchResponse>> {
    let mode = LyricsMode::parse(q.mode.as_deref());
    let page = q.page.as_deref().and_then(|s| s.parse::<i64>().ok());
    let limit = q.limit.as_deref().and_then(|s| s.parse::<i64>().ok());
    Ok(Json(
        st.vibe
            .lyrics(&q.q.unwrap_or_default(), mode, page, limit)
            .await?,
    ))
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
