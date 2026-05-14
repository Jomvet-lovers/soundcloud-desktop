use std::collections::HashSet;
use tracing::warn;

use super::service::{RecommendResult, RecommendationsService};

const TWO_TOWER_FEATURES: usize = 8;
const FALLBACK_TIMEOUT_QUIET: bool = true;

/// Two-tower rerank: backend assembles an 8-feature vector per candidate, ships
/// the batch to the worker, and rewrites `score` with the model output.
/// On any worker failure / timeout we silently keep the original score.
impl RecommendationsService {
    pub async fn apply_two_tower(
        &self,
        items: &mut [RecommendResult],
        recent_artists: &HashSet<String>,
    ) {
        if items.is_empty() {
            return;
        }
        let mut features: Vec<Vec<f32>> = Vec::with_capacity(items.len());
        for it in items.iter() {
            features.push(build_two_tower_features(it, recent_artists));
        }

        let scores = match self.worker.score_two_tower(&features).await {
            Ok(Some(s)) if s.len() == items.len() => s,
            Ok(_) => return,
            Err(e) => {
                if !FALLBACK_TIMEOUT_QUIET {
                    warn!(error = %e, "two_tower score failed");
                }
                return;
            }
        };
        for (it, s) in items.iter_mut().zip(scores.into_iter()) {
            it.score = Some(s);
        }
        items.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

fn build_two_tower_features(item: &RecommendResult, recent_artists: &HashSet<String>) -> Vec<f32> {
    let base = item.features.as_deref().unwrap_or(&[]);
    let mut f = vec![0f32; TWO_TOWER_FEATURES];
    for i in 0..base.len().min(6) {
        f[i] = base[i];
    }
    let artist_key = item
        .artist
        .as_deref()
        .map(|a| a.to_lowercase())
        .unwrap_or_default();
    f[6] = if artist_key.is_empty() || recent_artists.contains(&artist_key) {
        0.0
    } else {
        1.0
    };
    f[7] = match item.playback_count {
        Some(p) if p > 0 => ((p as f64).ln_1p() as f32) / 16.0,
        _ => 0.0,
    };
    f
}
