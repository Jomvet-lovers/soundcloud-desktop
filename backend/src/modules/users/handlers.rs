use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::cache::cache_service::CacheScope;
use crate::cache::ListPageResult;
use crate::common::cache_helper::cached_or_fetch;
use crate::common::pagination::PaginationQuery;
use crate::common::sc_ids::extract_sc_id;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::enrich::dto as enrich_dto;
use crate::modules::me::service::premium_response;
use crate::state::AppState;

// `/users/{my_urn}/*` и `/me/*` — одна и та же mirror-таблица. UsersService
// сам разрулит is_self → /me/ vs /users/{id}/ path + правильный TokenKind.

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(search))
        .route("/users/{user_urn}", get(get_by_id))
        .route("/users/{user_urn}/followers", get(get_followers))
        .route("/users/{user_urn}/followings", get(get_followings))
        .route(
            "/users/{user_urn}/followings/{following_urn}",
            get(get_is_following),
        )
        .route("/users/{user_urn}/tracks", get(get_tracks))
        .route("/users/{user_urn}/playlists", get(get_playlists))
        .route("/users/{user_urn}/likes/tracks", get(get_liked_tracks))
        .route(
            "/users/{user_urn}/likes/playlists",
            get(get_liked_playlists),
        )
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

async fn search(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(
        st.users
            .search(ctx.session_id, page, limit, q.q, q.ids)
            .await?,
    ))
}

async fn get_by_id(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(st.users.get_by_id(ctx.session_id, &user_urn).await?))
}

async fn get_followers(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(
        st.users
            .get_followers(ctx.session_id, &user_urn, page, limit)
            .await?,
    ))
}

async fn get_followings(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let target = extract_sc_id(&user_urn);
    Ok(Json(
        st.users
            .get_followings(ctx.session_id, &ctx.sc_user_id, target, page, limit)
            .await?,
    ))
}

async fn get_is_following(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path((user_urn, following_urn)): Path<(String, String)>,
) -> AppResult<Response> {
    let url = format!("/users/{user_urn}/followings/{following_urn}");
    cached_or_fetch(
        &st,
        "GET",
        &url,
        CacheScope::Shared,
        None,
        30,
        None,
        || async {
            let v = st
                .users
                .get_is_following(ctx.session_id, &user_urn, &following_urn)
                .await?;
            Ok(Value::Bool(v))
        },
    )
    .await
}

async fn get_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
    Query(_q): Query<AccessQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let target = extract_sc_id(&user_urn);
    let mut result = st
        .users
        .get_owned_tracks(ctx.session_id, &ctx.sc_user_id, target, page, limit)
        .await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut result.collection).await?;
    Ok(Json(result))
}

async fn get_playlists(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let target = extract_sc_id(&user_urn);
    Ok(Json(
        st.users
            .get_owned_playlists(ctx.session_id, &ctx.sc_user_id, target, page, limit)
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
    let access = q
        .access
        .unwrap_or_else(|| "playable,preview,blocked".into());
    let target = extract_sc_id(&user_urn);
    let mut result = st
        .users
        .get_liked_tracks(
            ctx.session_id,
            &ctx.sc_user_id,
            target,
            page,
            limit,
            &access,
        )
        .await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut result.collection).await?;
    Ok(Json(result))
}

async fn get_liked_playlists(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let target = extract_sc_id(&user_urn);
    Ok(Json(
        st.users
            .get_liked_playlists(ctx.session_id, &ctx.sc_user_id, target, page, limit)
            .await?,
    ))
}

async fn get_subscription(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Response> {
    let url = format!("/users/{user_urn}/subscription");
    cached_or_fetch(
        &st,
        "GET",
        &url,
        CacheScope::Shared,
        None,
        300,
        None,
        || async {
            let premium = st.subscriptions.is_premium(&user_urn).await?;
            Ok(premium_response(premium))
        },
    )
    .await
}

async fn get_web_profiles(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Response> {
    let url = format!("/users/{user_urn}/web-profiles");
    cached_or_fetch(
        &st,
        "GET",
        &url,
        CacheScope::Shared,
        None,
        86400,
        None,
        || async { st.users.get_web_profiles(ctx.session_id, &user_urn).await },
    )
    .await
}
