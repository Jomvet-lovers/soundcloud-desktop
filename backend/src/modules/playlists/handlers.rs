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
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/playlists", get(search).post(create))
        .route(
            "/playlists/{playlist_urn}",
            get(get_by_id).put(update_playlist).delete(delete_playlist),
        )
        .route("/playlists/{playlist_urn}/tracks", get(get_tracks))
        .route("/playlists/{playlist_urn}/reposters", get(get_reposters))
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

#[derive(Debug, Clone, Deserialize)]
struct TracksQuery {
    #[serde(default)]
    secret_token: Option<String>,
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
            .search(&ctx.access_token, page, limit, extra)
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
        .create(&ctx.access_token, &ctx.session_id.to_string(), &body)
        .await?;
    let _ = st
        .cache
        .clear_by_cache_keys(&["me-playlists".into()], Some(&ctx.session_id.to_string()))
        .await;
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
) -> AppResult<Response> {
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
    let url = build_url(&format!("/playlists/{playlist_urn}"), &params);
    let cache_key = format!("playlist-detail:{playlist_urn}");
    cached_or_fetch(
        &st,
        "GET",
        &url,
        CacheScope::Shared,
        None,
        3600,
        Some(&cache_key),
        || async {
            st.playlists
                .get_by_id(&ctx.access_token, &playlist_urn, &params)
                .await
        },
    )
    .await
}

async fn update_playlist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(playlist_urn): Path<String>,
    Json(body): Json<Value>,
) -> AppResult<Json<Value>> {
    let v = st
        .playlists
        .update(
            &ctx.access_token,
            &ctx.session_id.to_string(),
            &playlist_urn,
            &body,
        )
        .await?;
    let detail_key = format!("playlist-detail:{playlist_urn}");
    let tracks_key = format!("playlist-tracks:{playlist_urn}");
    let exact_keys = vec![detail_key.clone(), tracks_key.clone()];
    let session_id = ctx.session_id.to_string();
    let _ = st
        .cache
        .clear_by_cache_keys(
            &[exact_keys.clone(), vec!["me-playlists".into()]].concat(),
            Some(&session_id),
        )
        .await;
    let _ = st
        .list_cache
        .invalidate_by_cache_keys(&exact_keys, Some(&session_id))
        .await;
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
    let v = st
        .playlists
        .delete(
            &ctx.access_token,
            &ctx.session_id.to_string(),
            &playlist_urn,
        )
        .await?;
    let detail_key = format!("playlist-detail:{playlist_urn}");
    let tracks_key = format!("playlist-tracks:{playlist_urn}");
    let exact_keys = vec![detail_key.clone(), tracks_key.clone()];
    let session_id = ctx.session_id.to_string();
    let _ = st
        .cache
        .clear_by_cache_keys(
            &[
                exact_keys.clone(),
                vec!["me-playlists".into(), "me-liked-playlists".into()],
            ]
            .concat(),
            Some(&session_id),
        )
        .await;
    let _ = st
        .list_cache
        .invalidate_by_cache_keys(&exact_keys, Some(&session_id))
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
    Query(q): Query<TracksQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let mut extra: Vec<(String, String)> = vec![(
        "access".into(),
        q.access
            .unwrap_or_else(|| "playable,preview,blocked".into()),
    )];
    if let Some(v) = q.secret_token {
        extra.push(("secret_token".into(), v));
    }
    Ok(Json(
        st.playlists
            .get_tracks(&ctx.access_token, &playlist_urn, page, limit, extra)
            .await?,
    ))
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
            .get_reposters(&ctx.access_token, &playlist_urn, page, limit)
            .await?,
    ))
}

fn build_url(path: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return path.to_string();
    }
    let qs = serde_urlencoded::to_string(params).unwrap_or_default();
    if qs.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{qs}")
    }
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
    let payload =
        serde_json::to_string(&v).map_err(|e| AppError::internal(format!("json encode: {e}")))?;
    let _ = st
        .cache
        .set_raw(&key, &payload, ttl_sec, cache_key, scope, session_id)
        .await;
    Ok(json_response(StatusCode::OK, payload))
}
