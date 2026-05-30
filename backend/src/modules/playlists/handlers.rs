use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::cache::ListPageResult;
use crate::common::pagination::PaginationQuery;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::enrich::dto as enrich_dto;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/playlists", get(search).post(create))
        .route(
            "/playlists/{playlist_urn}",
            get(get_by_id).put(update_playlist).delete(delete_playlist),
        )
        .route("/playlists/{playlist_urn}/tracks", get(get_tracks))
        .route("/playlists/{playlist_urn}/sharing", put(set_playlist_sharing))
        .route("/playlists/{playlist_urn}/reposters", get(get_reposters))
}

#[derive(Debug, Clone, Deserialize)]
struct SharingBody {
    sharing: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    access: Option<String>,
    #[serde(default)]
    show_tracks: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DetailQuery {
    #[serde(default)]
    secret_token: Option<String>,
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
    let mut extra: Vec<(String, String)> = vec![(
        "access".into(),
        q.access
            .unwrap_or_else(|| "playable,preview,blocked".into()),
    )];
    if let Some(v) = q.q {
        extra.push(("q".into(), v));
    }
    if let Some(v) = q.show_tracks {
        extra.push(("show_tracks".into(), v));
    }
    Ok(Json(
        st.playlists
            .search(ctx.session_id, page, limit, extra)
            .await?,
    ))
}

async fn create(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Json(body): Json<Value>,
) -> AppResult<Json<Value>> {
    let v = st
        .playlists
        .create(ctx.session_id, &ctx.sc_user_id, &body)
        .await?;
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-playlists"], Some(&ctx.session_id.to_string()))
        .await;
    Ok(Json(v))
}

async fn get_by_id(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
    Query(q): Query<DetailQuery>,
) -> AppResult<Json<Value>> {
    let mut params: Vec<(String, String)> = vec![(
        "access".into(),
        q.access
            .unwrap_or_else(|| "playable,preview,blocked".into()),
    )];
    if let Some(v) = q.secret_token {
        params.push(("secret_token".into(), v));
    }
    if let Some(v) = q.show_tracks {
        params.push(("show_tracks".into(), v));
    }
    let mut value = st
        .playlists
        .get_by_id(ctx.session_id, &ctx.sc_user_id, &playlist_urn, &params)
        .await?;
    if let Some(arr) = value.get_mut("tracks").and_then(|v| v.as_array_mut()) {
        enrich_dto::apply_to_tracks(&st.pg, arr.as_mut_slice()).await?;
    }
    let mut single = vec![value];
    crate::modules::likes::cold::apply_user_favorite_flag_to_playlists(
        &st.pg,
        &ctx.sc_user_id,
        &mut single,
    )
    .await?;
    Ok(Json(single.into_iter().next().unwrap_or(Value::Null)))
}

async fn update_playlist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
    Json(body): Json<Value>,
) -> AppResult<Json<Value>> {
    let v = st
        .playlists
        .update(&ctx.sc_user_id, &playlist_urn, &body)
        .await?;
    let session_id = ctx.session_id.to_string();
    let tracks_key = format!("playlist-tracks:{playlist_urn}");
    let _ = st
        .list_cache
        .invalidate_by_cache_keys(&[tracks_key], Some(&session_id))
        .await;
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-playlists"], Some(&session_id))
        .await;
    Ok(Json(v))
}

async fn set_playlist_sharing(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
    Json(body): Json<SharingBody>,
) -> AppResult<Json<Value>> {
    let v = st
        .playlists
        .set_sharing(&ctx.sc_user_id, &playlist_urn, &body.sharing)
        .await?;
    let session_id = ctx.session_id.to_string();
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-playlists"], Some(&session_id))
        .await;
    Ok(Json(v))
}

async fn delete_playlist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
) -> AppResult<Json<Value>> {
    let v = st.playlists.delete(&ctx.sc_user_id, &playlist_urn).await?;
    let session_id = ctx.session_id.to_string();
    let tracks_key = format!("playlist-tracks:{playlist_urn}");
    let _ = st
        .list_cache
        .invalidate_by_cache_keys(&[tracks_key], Some(&session_id))
        .await;
    let _ = st
        .list_cache
        .invalidate_by_prefixes(&["me-playlists", "me-liked-playlists"], Some(&session_id))
        .await;
    Ok(Json(v))
}

async fn get_tracks(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let mut result = st
        .playlists
        .get_tracks(ctx.session_id, &ctx.sc_user_id, &playlist_urn, page, limit)
        .await?;
    enrich_dto::apply_to_tracks(&st.pg, &mut result.collection).await?;
    Ok(Json(result))
}

async fn get_reposters(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(
        st.playlists
            .get_reposters(ctx.session_id, &playlist_urn, page, limit)
            .await?,
    ))
}
