use qdrant_client::qdrant::SearchPointsBuilder;
use tracing::debug;

use crate::error::AppResult;
use crate::qdrant::collections;

use super::service::util::{payload_to_map, point_id_to_value, value_to_u64};
use super::service::{RecommendResult, RecommendationsService};

/// Длина LTR-features schema (исторически 8). Сейчас LTR-инференса нет, но
/// схема рассинхрона с rec_impressions ломает аналитику — держим как было.
const FEATURE_LEN: usize = 8;

impl RecommendationsService {
    pub async fn search_by_text(
        &self,
        query: &str,
        limit: usize,
        languages: Option<&[String]>,
    ) -> AppResult<Vec<RecommendResult>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let vec = match self.worker.encode_text_mulan(q).await {
            Ok(Some(v)) if !v.is_empty() => v,
            _ => return Ok(Vec::new()),
        };
        let filter = self.build_filter(&[], languages);
        let fetch_limit = (limit * 3).max(40);

        let mut builder =
            SearchPointsBuilder::new(collections::TRACKS_CLAP, vec, fetch_limit as u64)
                .with_payload(true);
        if let Some(f) = filter {
            builder = builder.filter(f);
        }
        let resp = match self.qdrant.raw().search_points(builder).await {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "searchByText: qdrant search failed");
                return Ok(Vec::new());
            }
        };

        let scored: Vec<super::service::ScoredCandidate> = resp
            .result
            .into_iter()
            .filter_map(|p| {
                let id_val = point_id_to_value(p.id);
                let id = value_to_u64(&id_val)?;
                Some(super::service::ScoredCandidate {
                    id,
                    score: p.score,
                    payload: Some(payload_to_map(p.payload)),
                    features: vec![0.0; FEATURE_LEN],
                })
            })
            .collect();

        let enriched = self.enrich_and_boost(scored, languages).await?;
        let diverse = self.artist_cap(enriched, self.cfg.artist_cap);
        self.take_verified(diverse, limit).await
    }
}
