use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::Value;

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/reposts/tracks/{track_urn}", post(repost_track).delete(remove_track_repost))
        .route("/reposts/playlists/{playlist_urn}", post(repost_playlist).delete(remove_playlist_repost))
}

async fn repost_track(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
) -> AppResult<impl IntoResponse> {
    let v = st.reposts.repost_track(&ctx.access_token, &ctx.session_id.to_string(), &track_urn).await?;
    Ok((StatusCode::CREATED, Json(v)))
}

async fn remove_track_repost(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        st.reposts.remove_track_repost(&ctx.access_token, &ctx.session_id.to_string(), &track_urn).await?,
    ))
}

async fn repost_playlist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
) -> AppResult<impl IntoResponse> {
    let v = st.reposts.repost_playlist(&ctx.access_token, &ctx.session_id.to_string(), &playlist_urn).await?;
    Ok((StatusCode::CREATED, Json(v)))
}

async fn remove_playlist_repost(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(
        st.reposts.remove_playlist_repost(&ctx.access_token, &ctx.session_id.to_string(), &playlist_urn).await?,
    ))
}
