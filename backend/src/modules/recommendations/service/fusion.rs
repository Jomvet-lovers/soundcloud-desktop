use std::collections::HashSet;
use tracing::{info, warn};

use crate::common::user_id::user_id_to_qdrant_id;
use crate::error::AppResult;

use super::types::{RecommendResult, ScoredCandidate, SeedVectors, WaveMode};
use super::util::parse_id_or_null;
use super::RecommendationsService;

const DIVERSE_DIVERSITY: f32 = 0.7;

impl RecommendationsService {
    /// Personalised wave fusion: combines user-taste, collab and an optional
    /// anchor track. Used by the "for_you" home cluster and the infinite tail.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn wave_fusion(
        &self,
        sc_user_id: &str,
        sc_track_id: Option<&str>,
        positive: &[String],
        negative: &[String],
        exclude: &[String],
        limit: usize,
        languages: Option<&[String]>,
        mode: WaveMode,
        req_id: &str,
    ) -> AppResult<Vec<RecommendResult>> {
        let anchor = sc_track_id.and_then(parse_id_or_null);
        let positive_ids: Vec<u64> = positive
            .iter()
            .filter_map(|s| parse_id_or_null(s))
            .collect();
        let negative_ids: Vec<u64> = negative
            .iter()
            .filter_map(|s| parse_id_or_null(s))
            .collect();

        let div = if mode == WaveMode::Diverse {
            DIVERSE_DIVERSITY
        } else {
            0.0
        };
        let fetch_limit: usize = if mode == WaveMode::Diverse {
            (limit * 20).max(500)
        } else {
            (limit * 12).max(300)
        };
        let threshold = (self.cfg.score_threshold as f32 - div * 0.04).max(0.0);

        let user_taste_id = user_id_to_qdrant_id(sc_user_id);
        let taste_fut = self.load_user_taste_vectors(user_taste_id);
        let collab_fut = async { self.collab.get_user_vector(sc_user_id).await };
        let (taste, user_collab) = tokio::join!(taste_fut, collab_fut);
        let user_collab = user_collab?;
        let seed = SeedVectors {
            collab: user_collab.clone(),
            mert: taste.0,
            clap: taste.1,
            lyrics: taste.2,
        };
        info!(
            req_id,
            mode = ?mode,
            anchor = ?anchor,
            pos = positive_ids.len(),
            neg = negative_ids.len(),
            excl = exclude.len(),
            limit,
            fetch_limit,
            threshold,
            "wave_fusion start"
        );

        let mut candidate_ids = self
            .build_candidate_pool(
                user_collab.as_deref(),
                seed.mert.is_some(),
                user_taste_id,
                anchor,
                &positive_ids,
                &negative_ids,
                exclude,
                languages,
                fetch_limit,
                req_id,
            )
            .await;
        let coplay_ids = self
            .coplay_arm(sc_user_id, anchor, exclude, fetch_limit / 4, req_id)
            .await;
        if !coplay_ids.is_empty() {
            let mut existing: HashSet<u64> = candidate_ids.iter().copied().collect();
            for id in coplay_ids {
                if existing.insert(id) {
                    candidate_ids.push(id);
                }
            }
        }
        if candidate_ids.is_empty() {
            warn!(req_id, "wave_fusion: empty pool, fallback");
            return self.get_fallback_tracks(exclude, limit, languages).await;
        }

        let scored = self.score_by_all_bases(&candidate_ids, &seed, req_id).await;
        let filtered: Vec<ScoredCandidate> = scored
            .into_iter()
            .filter(|s| s.score >= threshold)
            .collect();
        info!(req_id, scored = filtered.len(), "wave_fusion scored");

        let mut enriched = self.enrich_and_boost(filtered, languages).await?;
        self.apply_two_tower(&mut enriched, &std::collections::HashSet::new())
            .await;
        let reranked_count = enriched.len().min(limit * 4);
        let reranked = self
            .apply_ltr_rerank(enriched, reranked_count, req_id)
            .await;
        let ranked = if div > 0.0 {
            let work = reranked.len().min(limit * 8);
            self.apply_mmr(reranked, div, work).await
        } else {
            reranked
        };
        let diverse = self.artist_cap(ranked, self.cfg.artist_cap);
        let verified = self.take_verified(diverse, limit).await?;
        if verified.len() >= 5 {
            return Ok(verified);
        }
        warn!(
            req_id,
            count = verified.len(),
            "wave_fusion too few, fallback"
        );
        self.get_fallback_tracks(exclude, limit, languages).await
    }
}
