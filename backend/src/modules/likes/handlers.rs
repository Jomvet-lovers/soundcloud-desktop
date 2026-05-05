use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/likes/tracks/{track_urn}",
            post(like_track).delete(unlike_track),
        )
        .route(
            "/likes/playlists/{playlist_urn}",
            post(like_playlist).delete(unlike_playlist).get(is_playlist_liked),
        )
}

async fn like_track(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    body: Option<Json<Value>>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let track_data = body.map(|Json(v)| v);
    let v = st
        .likes
        .like_track(
            &ctx.access_token,
            &ctx.sc_user_id,
            &ctx.session_id.to_string(),
            &track_urn,
            track_data.as_ref(),
        )
        .await?;
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-liked-tracks"], Some(&ctx.session_id.to_string()))
        .await;
    Ok((StatusCode::OK, Json(v)))
}

async fn unlike_track(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
) -> AppResult<Json<Value>> {
    let v = st
        .likes
        .unlike_track(
            &ctx.access_token,
            &ctx.sc_user_id,
            &ctx.session_id.to_string(),
            &track_urn,
        )
        .await?;
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-liked-tracks"], Some(&ctx.session_id.to_string()))
        .await;
    Ok(Json(v))
}

async fn like_playlist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let v = st
        .likes
        .like_playlist(&ctx.access_token, &ctx.session_id.to_string(), &playlist_urn)
        .await?;
    let session_id = ctx.session_id.to_string();
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-liked-playlists"], Some(&session_id))
        .await;
    let _ = st
        .cache
        .clear_by_cache_keys(
            &[format!("playlist-liked-check:{playlist_urn}")],
            Some(&session_id),
        )
        .await;
    Ok((StatusCode::OK, Json(v)))
}

async fn unlike_playlist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
) -> AppResult<Json<Value>> {
    let v = st
        .likes
        .unlike_playlist(&ctx.access_token, &ctx.session_id.to_string(), &playlist_urn)
        .await?;
    let session_id = ctx.session_id.to_string();
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-liked-playlists"], Some(&session_id))
        .await;
    let _ = st
        .cache
        .clear_by_cache_keys(
            &[format!("playlist-liked-check:{playlist_urn}")],
            Some(&session_id),
        )
        .await;
    Ok(Json(v))
}

async fn is_playlist_liked(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        st.likes.is_playlist_liked(&ctx.access_token, &playlist_urn).await?,
    ))
}
