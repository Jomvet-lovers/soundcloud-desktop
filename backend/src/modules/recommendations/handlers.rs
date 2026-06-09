use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::common::query::parse_languages;
use crate::common::session::SessionCtx;
use crate::error::AppResult;
use crate::modules::recommendations::home_wave::HomeRequest;
use crate::modules::recommendations::service::RecommendResult;
use crate::modules::recommendations::smart_wave::{
    self, SmartWaveRequest, SmartWaveResponse, SmartWaveSeed,
};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/recommendations", get(home))
        .route("/recommendations/similar/{track_id}", get(similar))
        .route("/recommendations/artist/{artist_id}", get(artist))
        .route("/recommendations/search", get(search))
        .route("/recommendations/feedback", post(feedback))
        .route("/recommendations/wave", get(wave_user))
        .route(
            "/recommendations/wave/from-track/{seed_track_id}",
            get(wave_track),
        )
        .route(
            "/recommendations/wave/from-artist/{artist_id}",
            get(wave_artist),
        )
        .route("/recommendations/wave/feedback", post(wave_feedback))
}

fn parse_limit(raw: Option<&str>, fallback: usize) -> usize {
    raw.and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(fallback)
}

/// Булев query-флаг. Дефолт ON; `?flag=0/false/no` → OFF.
fn parse_flag(raw: Option<&str>, default: bool) -> bool {
    match raw {
        Some(s) => !matches!(s.trim(), "0" | "false" | "no"),
        None => default,
    }
}

#[derive(Debug, Deserialize)]
struct HomeQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
    #[serde(default)]
    hide_listened: Option<String>,
}

async fn home(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<HomeQuery>,
) -> AppResult<Response> {
    if ctx.sc_user_id.is_empty() {
        return Ok(
            Json(crate::modules::recommendations::clusters::ClusterBuilder::new().finish())
                .into_response(),
        );
    }
    let per_cluster = parse_limit(q.limit.as_deref(), 16);
    let languages = parse_languages(q.languages.as_deref());
    let req = HomeRequest {
        sc_user_id: ctx.sc_user_id.clone(),
        languages,
        per_cluster,
        hide_listened: parse_flag(q.hide_listened.as_deref(), true),
    };
    // Кэшированный JSON (короткий TTL) — снимает повтор ANN-сборки кластеров.
    let json = st.recommendations.home_wave_cached(req).await?;
    Ok(([(header::CONTENT_TYPE, "application/json")], json).into_response())
}

#[derive(Debug, Deserialize)]
struct SimilarQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
    #[serde(default)]
    hide_listened: Option<String>,
}

async fn similar(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(track_id): Path<String>,
    Query(q): Query<SimilarQuery>,
) -> AppResult<Response> {
    let per_cluster = parse_limit(q.limit.as_deref(), 12);
    let languages = parse_languages(q.languages.as_deref());
    let json = st
        .recommendations
        .similar_wave_cached(
            &track_id,
            &ctx.sc_user_id,
            languages.as_deref(),
            per_cluster,
            parse_flag(q.hide_listened.as_deref(), true),
        )
        .await?;
    Ok(([(header::CONTENT_TYPE, "application/json")], json).into_response())
}

#[derive(Debug, Deserialize)]
struct ArtistQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    hide_listened: Option<String>,
}

async fn artist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(artist_id): Path<Uuid>,
    Query(q): Query<ArtistQuery>,
) -> AppResult<Response> {
    let per_cluster = parse_limit(q.limit.as_deref(), 14);
    let json = st
        .recommendations
        .artist_wave_cached(
            artist_id,
            &ctx.sc_user_id,
            per_cluster,
            parse_flag(q.hide_listened.as_deref(), true),
        )
        .await?;
    Ok(([(header::CONTENT_TYPE, "application/json")], json).into_response())
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
    Ok(Json(out.results))
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

#[derive(Debug, Deserialize)]
struct WaveQuery {
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    languages: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    hide_listened: Option<String>,
}

#[derive(Debug, Serialize)]
struct WavePayload {
    tracks: Vec<RecommendResult>,
    cursor: String,
}

async fn wave_user(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Query(q): Query<WaveQuery>,
) -> AppResult<Json<WavePayload>> {
    let limit = parse_limit(q.limit.as_deref(), 20).clamp(4, 40);
    let languages = parse_languages(q.languages.as_deref());
    if ctx.sc_user_id.is_empty() {
        return Ok(Json(WavePayload {
            tracks: Vec::new(),
            cursor: String::new(),
        }));
    }
    let req = SmartWaveRequest {
        sc_user_id: &ctx.sc_user_id,
        languages: languages.as_deref(),
        limit,
        cursor_token: q.cursor.as_deref(),
        seed: SmartWaveSeed::User,
        hide_listened: parse_flag(q.hide_listened.as_deref(), true),
    };
    let SmartWaveResponse { tracks, cursor } = smart_wave::build(&st.recommendations, req).await?;
    Ok(Json(WavePayload { tracks, cursor }))
}

async fn wave_track(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(seed_track_id): Path<String>,
    Query(q): Query<WaveQuery>,
) -> AppResult<Json<WavePayload>> {
    let limit = parse_limit(q.limit.as_deref(), 20).clamp(4, 40);
    let languages = parse_languages(q.languages.as_deref());
    let Ok(seed) = seed_track_id.parse::<u64>() else {
        return Ok(Json(WavePayload {
            tracks: Vec::new(),
            cursor: String::new(),
        }));
    };
    let req = SmartWaveRequest {
        sc_user_id: &ctx.sc_user_id,
        languages: languages.as_deref(),
        limit,
        cursor_token: q.cursor.as_deref(),
        seed: SmartWaveSeed::Track(seed),
        hide_listened: parse_flag(q.hide_listened.as_deref(), true),
    };
    let SmartWaveResponse { tracks, cursor } = smart_wave::build(&st.recommendations, req).await?;
    Ok(Json(WavePayload { tracks, cursor }))
}

async fn wave_artist(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Path(artist_id): Path<Uuid>,
    Query(q): Query<WaveQuery>,
) -> AppResult<Json<WavePayload>> {
    let limit = parse_limit(q.limit.as_deref(), 20).clamp(4, 40);
    let languages = parse_languages(q.languages.as_deref());
    let top_tracks = st
        .recommendations
        .load_artist_top_track_ids(artist_id, 20)
        .await
        .unwrap_or_default();
    let req = SmartWaveRequest {
        sc_user_id: &ctx.sc_user_id,
        languages: languages.as_deref(),
        limit,
        cursor_token: q.cursor.as_deref(),
        seed: SmartWaveSeed::Artist(artist_id, &top_tracks),
        hide_listened: parse_flag(q.hide_listened.as_deref(), true),
    };
    let SmartWaveResponse { tracks, cursor } = smart_wave::build(&st.recommendations, req).await?;
    Ok(Json(WavePayload { tracks, cursor }))
}

#[derive(Debug, Deserialize)]
struct WaveFeedbackDto {
    cursor: String,
    #[serde(default)]
    negatives: usize,
    #[serde(default)]
    positives: usize,
}

#[derive(Debug, Serialize)]
struct WaveFeedbackResponse {
    ok: bool,
    cursor: Option<String>,
}

async fn wave_feedback(
    State(st): State<AppState>,
    ctx: SessionCtx,
    Json(body): Json<WaveFeedbackDto>,
) -> AppResult<Json<WaveFeedbackResponse>> {
    if body.cursor.is_empty() {
        return Ok(Json(WaveFeedbackResponse {
            ok: false,
            cursor: None,
        }));
    }
    let new_cursor = smart_wave::record_feedback(
        &st.recommendations,
        &ctx.sc_user_id,
        &body.cursor,
        body.negatives,
        body.positives,
    )
    .await;
    Ok(Json(WaveFeedbackResponse {
        ok: true,
        cursor: new_cursor,
    }))
}
