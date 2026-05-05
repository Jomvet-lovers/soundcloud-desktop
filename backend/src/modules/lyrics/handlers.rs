use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::AppResult;
use crate::modules::lyrics::service::{LyricsHints, LyricsResponse};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/lyrics/search", get(search))
        .route("/lyrics/{sc_track_id}", get(get_one))
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    duration: Option<String>,
}

fn parse_duration(d: Option<&str>) -> Option<i64> {
    let s = d?;
    let parsed: f64 = s.parse().ok()?;
    if !parsed.is_finite() {
        return None;
    }
    if parsed >= 10_000.0 {
        Some((parsed / 1000.0).round() as i64)
    } else {
        Some(parsed.round() as i64)
    }
}

async fn search(
    State(st): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<LyricsResponse>> {
    let hints = LyricsHints {
        artist: q.artist.unwrap_or_default(),
        title: q.title.unwrap_or_default(),
        duration_sec: parse_duration(q.duration.as_deref()),
    };
    Ok(Json(st.lyrics.search_lyrics(&hints).await?))
}

async fn get_one(
    State(st): State<AppState>,
    Path(sc_track_id): Path<String>,
) -> AppResult<Json<LyricsResponse>> {
    Ok(Json(st.lyrics.ensure_lyrics(&sc_track_id).await?))
}
