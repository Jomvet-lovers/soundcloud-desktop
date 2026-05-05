use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::cache::cache_service::CacheScope;
use crate::cache::ListPageResult;
use crate::common::pagination::PaginationQuery;
use crate::common::response::json_response;
use crate::common::session::SessionCtx;
use crate::error::{AppError, AppResult};
use crate::modules::me::service::premium_response;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(search))
        .route("/users/{user_urn}", get(get_by_id))
        .route("/users/{user_urn}/followers", get(get_followers))
        .route("/users/{user_urn}/followings", get(get_followings))
        .route("/users/{user_urn}/followings/{following_urn}", get(get_is_following))
        .route("/users/{user_urn}/tracks", get(get_tracks))
        .route("/users/{user_urn}/playlists", get(get_playlists))
        .route("/users/{user_urn}/likes/tracks", get(get_liked_tracks))
        .route("/users/{user_urn}/likes/playlists", get(get_liked_playlists))
        .route("/users/{user_urn}/subscription", get(get_subscription))
        .route("/users/{user_urn}/web-profiles", get(get_web_profiles))
}

#[derive(Debug, Clone, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    ids: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AccessQuery {
    #[serde(default)]
    access: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PlaylistsQuery {
    #[serde(default)]
    access: Option<String>,
    #[serde(default)]
    show_tracks: Option<String>,
}

async fn search(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(st.users.search(&ctx.access_token, page, limit, q.q, q.ids).await?))
}

async fn get_by_id(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Response> {
    let url = format!("/users/{user_urn}");
    cached_or_fetch(&st, "GET", &url, CacheScope::Shared, None, 3600, None, || async {
        st.users.get_by_id(&ctx.access_token, &user_urn).await
    })
    .await
}

async fn get_followers(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(st.users.get_followers(&ctx.access_token, &user_urn, page, limit).await?))
}

async fn get_followings(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(st.users.get_followings(&ctx.access_token, &user_urn, page, limit).await?))
}

async fn get_is_following(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path((user_urn, following_urn)): Path<(String, String)>,
) -> AppResult<Response> {
    let url = format!("/users/{user_urn}/followings/{following_urn}");
    cached_or_fetch(&st, "GET", &url, CacheScope::Shared, None, 30, None, || async {
        let v = st.users.get_is_following(&ctx.access_token, &user_urn, &following_urn).await?;
        Ok(Value::Bool(v))
    })
    .await
}

async fn get_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<AccessQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let access = q.access.unwrap_or_else(|| "playable,preview,blocked".into());
    Ok(Json(
        st.users
            .get_tracks(&ctx.access_token, &ctx.sc_user_id, &user_urn, page, limit, &access)
            .await?,
    ))
}

async fn get_playlists(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<PlaylistsQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let access = q.access.unwrap_or_else(|| "playable,preview,blocked".into());
    Ok(Json(
        st.users
            .get_playlists(&ctx.access_token, &user_urn, page, limit, &access, q.show_tracks)
            .await?,
    ))
}

async fn get_liked_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<AccessQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let access = q.access.unwrap_or_else(|| "playable,preview,blocked".into());
    Ok(Json(
        st.users
            .get_liked_tracks(&ctx.access_token, &ctx.sc_user_id, &user_urn, page, limit, &access)
            .await?,
    ))
}

async fn get_liked_playlists(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(st.users.get_liked_playlists(&ctx.access_token, &user_urn, page, limit).await?))
}

async fn get_subscription(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Response> {
    let url = format!("/users/{user_urn}/subscription");
    cached_or_fetch(&st, "GET", &url, CacheScope::Shared, None, 300, None, || async {
        let premium = st.subscriptions.is_premium(&user_urn).await?;
        Ok(premium_response(premium))
    })
    .await
}

async fn get_web_profiles(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Response> {
    let url = format!("/users/{user_urn}/web-profiles");
    cached_or_fetch(&st, "GET", &url, CacheScope::Shared, None, 86400, None, || async {
        st.users.get_web_profiles(&ctx.access_token, &user_urn).await
    })
    .await
}

async fn cached_or_fetch<F, Fut>(
    st: &AppState,
    method: &str,
    url: &str,
    scope: CacheScope,
    session_id: Option<&str>,
    ttl_sec: u64,
    cache_key: Option<&str>,
    fetch: F,
) -> AppResult<Response>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Value>>,
{
    let key = st.cache.build_key(method, url, scope, session_id);
    if let Ok(Some(raw)) = st.cache.get_raw(&key).await {
        return Ok(json_response(StatusCode::OK, raw));
    }
    let v = fetch().await?;
    let payload = serde_json::to_string(&v)
        .map_err(|e| AppError::internal(format!("json encode: {e}")))?;
    let _ = st.cache.set_raw(&key, &payload, ttl_sec, cache_key, scope, session_id).await;
    Ok(json_response(StatusCode::OK, payload))
}
