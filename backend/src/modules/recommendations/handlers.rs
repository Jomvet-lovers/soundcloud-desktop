use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rand::Rng;
use serde::Deserialize;
use std::collections::HashSet;

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::recommendations::service::{RecommendResult, WaveMode};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recommendations", get(recommend))
        .route("/recommendations/wave/{seed_track_id}", get(wave))
        .route("/recommendations/similar/{track_id}", get(similar))
        .route("/recommendations/search", get(search_by_text))
}

fn new_req_id() -> String {
    let mut rng = rand::thread_rng();
    let n: u64 = rng.gen();
    format!("{:x}", n & 0xffff_ffff)
}

fn parse_languages(raw: Option<&str>) -> Option<Vec<String>> {
    let s = raw?;
    let v: Vec<String> = s.split(',').filter(|x| !x.is_empty()).map(String::from).collect();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn parse_limit(raw: Option<&str>, fallback: usize) -> usize {
    raw.and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(fallback)
}

fn parse_diversity(raw: Option<&str>) -> f32 {
    raw.and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 1.0))
        .unwrap_or(0.0)
}

#[derive(Debug, Deserialize)]
struct CommonQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

async fn load_wave_context(
    st: &AppState,
    sc_user_id: &str,
) -> AppResult<(Vec<String>, Vec<String>, Vec<String>)> {
    let liked_fut = st.events.get_recent_liked(sc_user_id, 5);
    let skipped_fut = st.events.get_recent_skipped(sc_user_id, 3);
    let played_fut = st.events.get_recent_played(sc_user_id, 50);
    let disliked_fut = async { st.dislikes.list_ids_by_user_id(sc_user_id, 200).await };
    let (liked, skipped, played, disliked) =
        tokio::join!(liked_fut, skipped_fut, played_fut, disliked_fut);
    let liked = liked?;
    let skipped = skipped?;
    let played = played?;
    let disliked = disliked?;
    let disliked_set: HashSet<String> = disliked.iter().cloned().collect();
    let positive: Vec<String> = liked.into_iter().filter(|id| !disliked_set.contains(id)).collect();
    let mut neg_set: HashSet<String> = HashSet::new();
    for id in skipped.iter().chain(disliked.iter()) {
        neg_set.insert(id.clone());
    }
    let negative: Vec<String> = neg_set.into_iter().collect();
    let mut excl_set: HashSet<String> = HashSet::new();
    for id in played.iter().chain(disliked.iter()) {
        excl_set.insert(id.clone());
    }
    let exclude: Vec<String> = excl_set.into_iter().collect();
    Ok((positive, negative, exclude))
}

async fn recommend(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<CommonQuery>,
) -> AppResult<Json<Vec<RecommendResult>>> {
    let req_id = new_req_id();
    if ctx.sc_user_id.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let limit = parse_limit(q.limit.as_deref(), 20);
    let languages = parse_languages(q.languages.as_deref());
    let mode = WaveMode::parse(q.mode.as_deref());
    let (positive, negative, exclude) = load_wave_context(&st, &ctx.sc_user_id).await?;
    let out = st
        .recommendations
        .recommend(
            &ctx.sc_user_id,
            &positive,
            &negative,
            &exclude,
            limit,
            languages.as_deref(),
            mode,
            &req_id,
        )
        .await?;
    Ok(Json(out))
}

async fn wave(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(seed_track_id): Path<String>,
    Query(q): Query<CommonQuery>,
) -> AppResult<Json<Vec<RecommendResult>>> {
    let req_id = new_req_id();
    if ctx.sc_user_id.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let limit = parse_limit(q.limit.as_deref(), 20);
    let languages = parse_languages(q.languages.as_deref());
    let mode = WaveMode::parse(q.mode.as_deref());
    let (positive, negative, mut exclude) = load_wave_context(&st, &ctx.sc_user_id).await?;
    if !exclude.contains(&seed_track_id) {
        exclude.push(seed_track_id.clone());
    }
    let out = st
        .recommendations
        .wave(
            &ctx.sc_user_id,
            Some(&seed_track_id),
            &positive,
            &negative,
            &exclude,
            limit,
            languages.as_deref(),
            mode,
            &req_id,
        )
        .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct SimilarQuery {
    #[serde(default)]
    exclude: Option<String>,
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
    #[serde(default)]
    diversity: Option<String>,
}

async fn similar(
    State(st): State<AppState>,
    Path(track_id): Path<String>,
    Query(q): Query<SimilarQuery>,
) -> AppResult<Json<Vec<RecommendResult>>> {
    let req_id = new_req_id();
    let client_excl: Vec<String> = q
        .exclude
        .as_deref()
        .map(|s| s.split(',').filter(|x| !x.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    let mut exclude_set: HashSet<String> = client_excl.into_iter().collect();
    exclude_set.insert(track_id.clone());
    let exclude: Vec<String> = exclude_set.into_iter().collect();
    let limit = parse_limit(q.limit.as_deref(), 10);
    let languages = parse_languages(q.languages.as_deref());
    let diversity = parse_diversity(q.diversity.as_deref());
    let out = st
        .recommendations
        .similar(&track_id, &exclude, limit, languages.as_deref(), diversity, &req_id)
        .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
}

async fn search_by_text(
    State(st): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Vec<RecommendResult>>> {
    let limit = parse_limit(q.limit.as_deref(), 20);
    let languages = parse_languages(q.languages.as_deref());
    let out = st
        .recommendations
        .search_by_text(&q.q.unwrap_or_default(), limit, languages.as_deref())
        .await?;
    Ok(Json(out))
}
