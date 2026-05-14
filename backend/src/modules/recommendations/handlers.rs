use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use rand::Rng;
use serde::Deserialize;
use std::collections::HashSet;
use uuid::Uuid;

use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::recommendations::clusters::ClusterResponse;
use crate::modules::recommendations::home_wave::HomeRequest;
use crate::modules::recommendations::service::{RecommendResult, WaveMode};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recommendations", get(home))
        .route("/recommendations/tail/{seed_track_id}", get(tail))
        .route("/recommendations/similar/{track_id}", get(similar))
        .route("/recommendations/artist/{artist_id}", get(artist))
        .route("/recommendations/search", get(search))
        .route("/recommendations/feedback", axum::routing::post(feedback))
}

fn new_req_id() -> String {
    let mut rng = rand::thread_rng();
    let n: u64 = rng.gen();
    format!("{:x}", n & 0xffff_ffff)
}

fn parse_languages(raw: Option<&str>) -> Option<Vec<String>> {
    let s = raw?;
    let v: Vec<String> = s
        .split(',')
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect();
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

#[derive(Debug, Deserialize)]
struct HomeQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
}

async fn home(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<HomeQuery>,
) -> AppResult<Json<ClusterResponse>> {
    if ctx.sc_user_id.is_empty() {
        return Ok(Json(
            crate::modules::recommendations::clusters::ClusterBuilder::new().finish(),
        ));
    }
    let per_cluster = parse_limit(q.limit.as_deref(), 12);
    let languages = parse_languages(q.languages.as_deref());
    let req = HomeRequest {
        sc_user_id: ctx.sc_user_id.clone(),
        languages,
        per_cluster,
    };
    let out = st.recommendations.home_wave(req).await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct TailQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

async fn tail(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(seed_track_id): Path<String>,
    Query(q): Query<TailQuery>,
) -> AppResult<Json<Vec<RecommendResult>>> {
    let req_id = new_req_id();
    if ctx.sc_user_id.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let limit = parse_limit(q.limit.as_deref(), 20);
    let languages = parse_languages(q.languages.as_deref());
    let mode = WaveMode::parse(q.mode.as_deref());

    let liked = st.events.get_recent_liked(&ctx.sc_user_id, 5).await?;
    let skipped = st.events.get_recent_skipped(&ctx.sc_user_id, 3).await?;
    let played = st.events.get_recent_played(&ctx.sc_user_id, 50).await?;
    let disliked = st
        .dislikes
        .list_ids_by_user_id(&ctx.sc_user_id, 200)
        .await?;

    let disliked_set: HashSet<String> = disliked.iter().cloned().collect();
    let positive: Vec<String> = liked
        .into_iter()
        .filter(|id| !disliked_set.contains(id))
        .collect();
    let mut neg_set: HashSet<String> = HashSet::new();
    for id in skipped.iter().chain(disliked.iter()) {
        neg_set.insert(id.clone());
    }
    let negative: Vec<String> = neg_set.into_iter().collect();
    let mut excl_set: HashSet<String> = HashSet::new();
    for id in played.iter().chain(disliked.iter()) {
        excl_set.insert(id.clone());
    }
    if !excl_set.contains(&seed_track_id) {
        excl_set.insert(seed_track_id.clone());
    }
    let exclude: Vec<String> = excl_set.into_iter().collect();

    let mut seq_session = played.clone();
    if !seq_session.contains(&seed_track_id) {
        seq_session.push(seed_track_id.clone());
    }
    let seq_pool = st
        .recommendations
        .sequential_next_pool(&seq_session, limit * 2)
        .await
        .unwrap_or_default();

    let fusion = st
        .recommendations
        .wave_fusion(
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

    let out = if seq_pool.is_empty() {
        fusion
    } else {
        let mut merged: Vec<RecommendResult> = Vec::with_capacity(limit);
        let mut seen: HashSet<String> = HashSet::new();
        let mut seq_iter = seq_pool.into_iter();
        let mut fusion_iter = fusion.into_iter();
        for _ in 0..limit {
            if let Some(s) = seq_iter.next() {
                let id = crate::modules::recommendations::clusters::recommend_id_str(&s.id);
                if !id.is_empty() && seen.insert(id) {
                    merged.push(s);
                    if merged.len() >= limit {
                        break;
                    }
                }
            }
            if let Some(f) = fusion_iter.next() {
                let id = crate::modules::recommendations::clusters::recommend_id_str(&f.id);
                if !id.is_empty() && seen.insert(id) {
                    merged.push(f);
                    if merged.len() >= limit {
                        break;
                    }
                }
            }
        }
        merged
    };

    let track_ids: Vec<String> = out
        .iter()
        .filter_map(|r| {
            let s = crate::modules::recommendations::clusters::recommend_id_str(&r.id);
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        })
        .collect();
    let tail_cluster = crate::modules::recommendations::clusters::Cluster {
        id: "tail",
        track_ids,
        neighbors: None,
    };
    crate::modules::recommendations::impressions::log_clusters_async(
        st.pg.clone(),
        ctx.sc_user_id.clone(),
        crate::modules::recommendations::impressions::ImpressionSource::Tail,
        &[tail_cluster],
        &std::collections::HashMap::new(),
    );

    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct SimilarQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
}

async fn similar(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(track_id): Path<String>,
    Query(q): Query<SimilarQuery>,
) -> AppResult<Json<ClusterResponse>> {
    let per_cluster = parse_limit(q.limit.as_deref(), 10);
    let languages = parse_languages(q.languages.as_deref());
    let out = st
        .recommendations
        .similar_wave(&track_id, languages.as_deref(), per_cluster)
        .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct ArtistQuery {
    #[serde(default)]
    limit: Option<String>,
}

async fn artist(
    State(st): State<AppState>,
    _ctx: SessionCtx,
    Path(artist_id): Path<Uuid>,
    Query(q): Query<ArtistQuery>,
) -> AppResult<Json<ClusterResponse>> {
    let per_cluster = parse_limit(q.limit.as_deref(), 12);
    let out = st
        .recommendations
        .artist_wave(artist_id, per_cluster)
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

#[derive(Debug, Deserialize)]
struct FeedbackDto {
    #[serde(rename = "clusterId")]
    cluster_id: String,
    #[serde(rename = "type")]
    kind: String,
}

async fn feedback(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Json(body): Json<FeedbackDto>,
) -> AppResult<Json<serde_json::Value>> {
    if ctx.sc_user_id.is_empty() || body.cluster_id.is_empty() {
        return Ok(Json(serde_json::json!({"ok": false})));
    }
    let (clicks, completes) = match body.kind.as_str() {
        "click" => (1, 0),
        "complete" => (0, 1),
        _ => return Ok(Json(serde_json::json!({"ok": false, "reason": "bad_type"}))),
    };
    crate::modules::recommendations::bandits::record_outcome(
        &st.pg,
        &ctx.sc_user_id,
        &body.cluster_id,
        clicks,
        completes,
    )
    .await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn search(
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
