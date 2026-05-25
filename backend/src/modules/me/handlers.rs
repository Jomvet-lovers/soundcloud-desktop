use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde_json::Value;

use crate::cache::ListPageResult;
use crate::common::pagination::PaginationQuery;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::enrich::dto as enrich_dto;
use crate::modules::me::dto::LikedTracksQuery;
use crate::modules::me::service::premium_response;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_profile))
        .route("/me/subscription", get(get_subscription))
        .route("/me/feed", get(get_feed))
        .route("/me/feed/tracks", get(get_feed_tracks))
        .route("/me/likes/tracks", get(get_liked_tracks))
        .route("/me/likes/playlists", get(get_liked_playlists))
        .route("/me/followings", get(get_followings))
        .route("/me/followings/tracks", get(get_followings_tracks))
        .route(
            "/me/followings/{user_urn}",
            put(follow_user).delete(unfollow_user),
        )
        .route("/me/followers", get(get_followers))
        .route("/me/playlists", get(get_playlists))
        .route("/me/tracks", get(get_tracks))
}

async fn get_profile(State(st): State<AppState>, ctx: SessionCtx) -> AppResult<Json<Value>> {
    Ok(Json(st.me.get_profile(&ctx.access_token).await?))
}

async fn get_subscription(State(st): State<AppState>, ctx: SessionCtx) -> AppResult<Json<Value>> {
    let premium = st.subscriptions.is_premium(&ctx.sc_user_id).await?;
    Ok(Json(premium_response(premium)))
}

async fn get_feed(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    Ok(Json(
        st.me
            .get_feed(
                &ctx.access_token,
                &ctx.session_id.to_string(),
                &ctx.sc_user_id,
                page,
                limit,
            )
            .await?,
    ))
}

async fn get_feed_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    let mut result = st
        .me
        .get_feed_tracks(
            &ctx.access_token,
            &ctx.session_id.to_string(),
            &ctx.sc_user_id,
            page,
            limit,
        )
        .await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut result.collection).await?;
    Ok(Json(result))
}

async fn get_liked_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
    Query(a): Query<LikedTracksQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    let access = a
        .access
        .unwrap_or_else(|| "playable,preview,blocked".into());
    let mut result = st
        .users
        .get_liked_tracks(
            ctx.session_id,
            &ctx.sc_user_id,
            &ctx.sc_user_id,
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
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    Ok(Json(
        st.users
            .get_liked_playlists(ctx.session_id, &ctx.sc_user_id, &ctx.sc_user_id, page, limit)
            .await?,
    ))
}

async fn get_followings(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    Ok(Json(
        st.users
            .get_followings(ctx.session_id, &ctx.sc_user_id, &ctx.sc_user_id, page, limit)
            .await?,
    ))
}

async fn get_followings_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    let mut result = st
        .me
        .get_followings_tracks(
            &ctx.access_token,
            &ctx.session_id.to_string(),
            &ctx.sc_user_id,
            page,
            limit,
        )
        .await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut result.collection).await?;
    Ok(Json(result))
}

async fn follow_user(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Json<Value>> {
    let v = st.me.follow_user(&ctx.sc_user_id, &user_urn).await?;
    // Сбросить накопительный кэш me-followings этой сессии.
    if let Err(e) = st
        .list_cache
        .invalidate_by_prefixes(&["me-followings"], Some(&ctx.session_id.to_string()))
        .await
    {
        tracing::warn!(error = %e, "list-cache invalidate failed");
    }
    Ok(Json(v))
}

async fn unfollow_user(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(user_urn): Path<String>,
) -> AppResult<Json<Value>> {
    let v = st.me.unfollow_user(&ctx.sc_user_id, &user_urn).await?;
    if let Err(e) = st
        .list_cache
        .invalidate_by_prefixes(&["me-followings"], Some(&ctx.session_id.to_string()))
        .await
    {
        tracing::warn!(error = %e, "list-cache invalidate failed");
    }
    Ok(Json(v))
}

async fn get_followers(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    Ok(Json(
        st.me
            .get_followers(&ctx.access_token, &ctx.session_id.to_string(), page, limit)
            .await?,
    ))
}

async fn get_playlists(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    Ok(Json(
        st.users
            .get_owned_playlists(ctx.session_id, &ctx.sc_user_id, &ctx.sc_user_id, page, limit)
            .await?,
    ))
}

async fn get_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = q.resolved();
    let mut result = st
        .users
        .get_owned_tracks(ctx.session_id, &ctx.sc_user_id, &ctx.sc_user_id, page, limit)
        .await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut result.collection).await?;
    Ok(Json(result))
}
