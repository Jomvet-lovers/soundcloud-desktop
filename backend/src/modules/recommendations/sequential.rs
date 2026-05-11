use qdrant_client::qdrant::SearchPointsBuilder;
use tracing::debug;

use crate::error::AppResult;
use crate::qdrant::collections;

use super::service::util::{payload_to_map, point_id_to_value};
use super::service::{RecommendResult, RecommendationsService};

const MAX_HISTORY: usize = 20;
const MERT_DIM: usize = 1024;

/// Sequential-aware next-track retrieval. Picks the last N MERT vectors of the
/// user's session, asks the worker GRU to predict the "next" vector, then runs
/// qdrant search for nearest tracks. Returns Vec::new() if anything goes wrong
/// — callers must have a non-sequential fallback.
impl RecommendationsService {
    pub async fn sequential_next_pool(
        &self,
        session_track_ids: &[String],
        limit: usize,
    ) -> AppResult<Vec<RecommendResult>> {
        if session_track_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let trimmed: Vec<String> = session_track_ids
            .iter()
            .rev()
            .take(MAX_HISTORY)
            .rev()
            .cloned()
            .collect();
        let numeric: Vec<u64> = trimmed
            .iter()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        if numeric.is_empty() {
            return Ok(Vec::new());
        }
        let vec_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric)
            .await;
        let session_vectors: Vec<Vec<f32>> = numeric
            .iter()
            .filter_map(|n| vec_map.get(&n.to_string()).cloned())
            .collect();
        if session_vectors.is_empty() {
            return Ok(Vec::new());
        }

        let pred = match self
            .worker
            .predict_next_track_vectors(&[session_vectors])
            .await
        {
            Ok(Some(mut v)) if !v.is_empty() => v.remove(0),
            _ => return Ok(Vec::new()),
        };
        if pred.len() != MERT_DIM {
            debug!(pred_len = pred.len(), "sequential: bad pred dim");
            return Ok(Vec::new());
        }

        let mut builder = SearchPointsBuilder::new(collections::TRACKS_MERT, pred, limit as u64)
            .with_payload(true);
        let exclude_set: std::collections::HashSet<String> =
            session_track_ids.iter().cloned().collect();
        let filter = self.build_filter(&exclude_set.iter().cloned().collect::<Vec<_>>(), None);
        if let Some(f) = filter {
            builder = builder.filter(f);
        }
        match self.qdrant.raw().search_points(builder).await {
            Ok(r) => Ok(r
                .result
                .into_iter()
                .map(|p| RecommendResult {
                    id: point_id_to_value(p.id),
                    score: Some(p.score),
                    payload: Some(payload_to_map(p.payload)),
                    artist: None,
                    genre: None,
                    playback_count: None,
                    features: None,
                })
                .collect()),
            Err(e) => {
                debug!(error = %e, "sequential: search failed");
                Ok(Vec::new())
            }
        }
    }
}
