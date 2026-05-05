use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use crate::cache::cache_service::CacheScope;
use crate::cache::ListPageResult;
use crate::common::pagination::PaginationQuery;
use crate::common::response::json_response;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tracks", get(search))
        .route("/tracks/{track_urn}", get(get_by_id).put(update_track).delete(delete_track))
        .route("/tracks/{track_urn}/streams", get(get_streams))
        .route("/tracks/{track_urn}/stream", get(proxy_stream))
        .route("/tracks/{track_urn}/comments", get(get_comments).post(create_comment))
        .route("/tracks/{track_urn}/favoriters", get(get_favoriters))
        .route("/tracks/{track_urn}/reposters", get(get_reposters))
        .route("/tracks/{track_urn}/related", get(get_related))
}

#[derive(Debug, Clone, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    ids: Option<String>,
    #[serde(default)]
    genres: Option<String>,
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    access: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SecretTokenQuery {
    #[serde(default)]
    secret_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AccessQuery {
    #[serde(default)]
    access: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StreamProxyQuery {
    #[serde(default)]
    secret_token: Option<String>,
    #[serde(default)]
    hq: Option<String>,
}

async fn search(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(p): Query<PaginationQuery>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let access = q.access.unwrap_or_else(|| "playable,preview,blocked".into());
    let mut extra: Vec<(String, String)> = vec![("access".into(), access)];
    if let Some(v) = q.q { extra.push(("q".into(), v)); }
    if let Some(v) = q.ids { extra.push(("ids".into(), v)); }
    if let Some(v) = q.genres { extra.push(("genres".into(), v)); }
    if let Some(v) = q.tags { extra.push(("tags".into(), v)); }
    Ok(Json(
        st.tracks.search(&ctx.access_token, &ctx.sc_user_id, page, limit, extra).await?,
    ))
}

async fn get_by_id(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Query(s): Query<SecretTokenQuery>,
) -> AppResult<Response> {
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(t) = s.secret_token {
        params.push(("secret_token".into(), t));
    }
    cached_or_fetch(&st, "GET", &request_url("/tracks", &track_urn, &params), CacheScope::Shared, None, 600, || async {
        st.tracks.get_by_id(&ctx.access_token, &ctx.sc_user_id, &track_urn, &params).await
    })
    .await
}

async fn update_track(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Json(body): Json<Value>,
) -> AppResult<Json<Value>> {
    Ok(Json(st.tracks.update(&ctx.access_token, &track_urn, &body).await?))
}

async fn delete_track(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(st.tracks.delete(&ctx.access_token, &track_urn).await?))
}

async fn get_streams(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Query(s): Query<SecretTokenQuery>,
) -> AppResult<Response> {
    let mut params: Vec<(String, String)> = Vec::new();
    if let Some(t) = s.secret_token {
        params.push(("secret_token".into(), t));
    }
    let url = request_url(&format!("/tracks/{track_urn}/streams"), "", &params);
    cached_or_fetch(&st, "GET", &url, CacheScope::Shared, None, 3600, || async {
        st.tracks.get_streams(&ctx.access_token, &track_urn, &params).await
    })
    .await
}

async fn proxy_stream(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Query(q): Query<StreamProxyQuery>,
) -> Response {
    let mut params: Vec<(String, String)> =
        vec![("session_id".into(), ctx.session_id.to_string())];
    if let Some(t) = q.secret_token { params.push(("secret_token".into(), t)); }
    if let Some(h) = q.hq { params.push(("hq".into(), h)); }
    let qs = serde_urlencoded::to_string(&params).unwrap_or_default();
    let url = format!(
        "{}/stream/{}?{qs}",
        st.config.streaming.service_url,
        urlencoding::encode(&track_urn),
    );
    Redirect::permanent(&url).into_response()
}

async fn get_comments(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(st.tracks.get_comments(&ctx.access_token, &track_urn, page, limit).await?))
}

async fn create_comment(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Json(body): Json<Value>,
) -> AppResult<Json<Value>> {
    let v = st
        .tracks
        .create_comment(&ctx.access_token, &ctx.session_id.to_string(), &track_urn, &body)
        .await?;
    let _ = st
        .cache
        .clear_by_cache_keys(&[format!("track-comments:{track_urn}")], None)
        .await;
    let _ = st
        .list_cache
        .invalidate_by_cache_keys(&[format!("track-comments:{track_urn}")], None)
        .await;
    Ok(Json(v))
}

async fn get_favoriters(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(st.tracks.get_favoriters(&ctx.access_token, &track_urn, page, limit).await?))
}

async fn get_reposters(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    Ok(Json(st.tracks.get_reposters(&ctx.access_token, &track_urn, page, limit).await?))
}

async fn get_related(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_urn): Path<String>,
    Query(p): Query<PaginationQuery>,
    Query(a): Query<AccessQuery>,
) -> AppResult<Json<ListPageResult<Value>>> {
    let (page, limit) = p.resolved();
    let access = a.access.unwrap_or_else(|| "playable,preview,blocked".into());
    Ok(Json(
        st.tracks
            .get_related(&ctx.access_token, &ctx.sc_user_id, &track_urn, page, limit, &access)
            .await?,
    ))
}

fn request_url(prefix: &str, suffix: &str, params: &[(String, String)]) -> String {
    let path = if suffix.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}/{suffix}")
    };
    if params.is_empty() {
        return path;
    }
    let qs = serde_urlencoded::to_string(params).unwrap_or_default();
    if qs.is_empty() { path } else { format!("{path}?{qs}") }
}

async fn cached_or_fetch<F, Fut>(
    st: &AppState,
    method: &str,
    url: &str,
    scope: CacheScope,
    session_id: Option<&str>,
    ttl_sec: u64,
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
        .map_err(|e| crate::error::AppError::internal(format!("json encode: {e}")))?;
    let _ = st.cache.set_raw(&key, &payload, ttl_sec, None, scope, session_id).await;
    Ok(json_response(StatusCode::OK, payload))
}
