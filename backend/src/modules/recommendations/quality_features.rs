use std::collections::HashMap;

use sqlx::PgPool;

#[derive(Debug, Clone, Default)]
pub struct TrackMeta {
    pub plays: i64,
    pub likes: i64,
    pub duration_ms: i64,
    pub title: String,
    pub has_genre: bool,
    pub is_preview: bool,
}

pub async fn load_track_meta(pg: &PgPool, ids: &[String]) -> HashMap<String, TrackMeta> {
    if ids.is_empty() {
        return HashMap::new();
    }
    let rows: Vec<(
        String,
        String,
        Option<String>,
        i32,
        Option<i64>,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT it.sc_track_id, it.title, it.genre, it.duration_ms, c.play_count, c.likes_count
             FROM tracks it
             LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
             WHERE it.sc_track_id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pg)
    .await
    .unwrap_or_default();

    let mut out = HashMap::with_capacity(rows.len());
    for (id, title, genre, duration_ms, plays, likes) in rows {
        let lower = title.to_lowercase();
        let has_genre = genre.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
        let is_preview = lower.contains("preview") || lower.contains("teaser");
        out.insert(
            id,
            TrackMeta {
                plays: plays.unwrap_or(0),
                likes: likes.unwrap_or(0),
                duration_ms: duration_ms as i64,
                title,
                has_genre,
                is_preview,
            },
        );
    }
    out
}

pub fn vec_stats(v: Option<&Vec<f32>>) -> (f32, f32) {
    let Some(v) = v else {
        return (0.0, 0.0);
    };
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let n = v.len() as f32;
    let mean = v.iter().sum::<f32>() / n;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n;
    (mean, var.sqrt())
}

pub const QUALITY_FEATURE_LEN: usize = 10;

/// 10-feature vector consumed by the worker quality LR + the Rust fallback
/// heuristic. Order is contract — do not reorder columns.
pub fn build_features(
    meta: &TrackMeta,
    mert_stats: (f32, f32),
    clap_stats: (f32, f32),
) -> Vec<f32> {
    vec![
        mert_stats.0,
        mert_stats.1,
        clap_stats.0,
        clap_stats.1,
        ((meta.plays as f64).ln_1p() as f32) / 16.0,
        ((meta.likes as f64).ln_1p() as f32) / 12.0,
        (meta.duration_ms as f32) / 60_000.0,
        (meta.title.len() as f32 / 100.0).min(1.0),
        if meta.has_genre { 1.0 } else { 0.0 },
        if meta.is_preview { 1.0 } else { 0.0 },
    ]
}

pub fn fallback_score(features: &[f32]) -> f32 {
    if features.len() < QUALITY_FEATURE_LEN {
        return 0.5;
    }
    let log_plays = features[4];
    let log_likes = features[5];
    let duration_min = features[6];
    let is_preview = features[9];
    let score = 0.4 * (log_plays * 16.0 / 6.0).tanh()
        + 0.3 * (log_likes * 12.0 / 4.0).tanh()
        + 0.2 * (1.0 - (((duration_min - 3.5).abs()) / 5.0).min(1.0))
        + 0.1 * (1.0 - is_preview);
    score.clamp(0.0, 1.0)
}
