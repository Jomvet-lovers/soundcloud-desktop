use tracing::info;

use crate::modules::ltr::LTR_FEATURE_COUNT;
use crate::modules::recommendations::mmr::{greedy_pick, max_cosine_to_selected};
use crate::qdrant::collections;

use super::types::RecommendResult;
use super::util::value_to_u64;
use super::RecommendationsService;

impl RecommendationsService {
    pub(crate) async fn apply_ltr_rerank(
        &self,
        items: Vec<RecommendResult>,
        work_limit: usize,
        req_id: &str,
    ) -> Vec<RecommendResult> {
        if !self.ltr.enabled() || items.len() <= 1 {
            return items;
        }
        let work_limit = work_limit.min(items.len());
        let (head, tail) = items.split_at(work_limit);
        let head_vec: Vec<RecommendResult> = head.to_vec();
        let tail_vec: Vec<RecommendResult> = tail.to_vec();
        let features: Vec<Vec<f32>> = head_vec
            .iter()
            .map(|it| {
                it.features
                    .clone()
                    .unwrap_or_else(|| vec![0.0; LTR_FEATURE_COUNT])
            })
            .collect();
        let scores = match self.ltr.score(&features).await {
            Some(s) => s,
            None => return [head_vec, tail_vec].concat(),
        };
        let mut reranked: Vec<RecommendResult> = head_vec
            .into_iter()
            .enumerate()
            .map(|(i, mut it)| {
                it.score = Some(scores[i]);
                it
            })
            .collect();
        reranked.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        info!(req_id, count = reranked.len(), "ltr-rerank applied");
        [reranked, tail_vec].concat()
    }

    pub(crate) async fn apply_mmr(
        &self,
        items: Vec<RecommendResult>,
        diversity: f32,
        work_limit: usize,
    ) -> Vec<RecommendResult> {
        if items.len() <= 1 {
            return items;
        }
        let lambda = (1.0 - diversity).clamp(0.0, 1.0);

        let work_limit = work_limit.min(items.len());
        let (head, tail) = items.split_at(work_limit);
        let head_vec: Vec<RecommendResult> = head.to_vec();
        let tail_vec: Vec<RecommendResult> = tail.to_vec();

        let numeric_ids: Vec<u64> = head_vec
            .iter()
            .filter_map(|it| value_to_u64(&it.id))
            .collect();
        if numeric_ids.is_empty() {
            return [head_vec, tail_vec].concat();
        }

        let vectors = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric_ids)
            .await;
        if vectors.len() < 2 {
            return [head_vec, tail_vec].concat();
        }

        let centroid = self.centroids.get(collections::TRACKS_MERT);
        let whiten = |v: &[f32]| -> Vec<f32> {
            match &centroid {
                Some(c) => {
                    let n = v.len().min(c.len());
                    let mut out = Vec::with_capacity(n);
                    for i in 0..n {
                        out.push(v[i] - c[i]);
                    }
                    out
                }
                None => v.to_vec(),
            }
        };

        let mut pool: Vec<RecommendResult> = head_vec
            .iter()
            .filter(|it| {
                value_to_u64(&it.id)
                    .map(|n| vectors.contains_key(&n.to_string()))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let no_vec: Vec<RecommendResult> = head_vec
            .into_iter()
            .filter(|it| {
                value_to_u64(&it.id)
                    .map(|n| !vectors.contains_key(&n.to_string()))
                    .unwrap_or(true)
            })
            .collect();
        pool.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let pool_vecs: Vec<Vec<f32>> = pool
            .iter()
            .map(|it| {
                let id = value_to_u64(&it.id).unwrap();
                whiten(vectors.get(&id.to_string()).unwrap())
            })
            .collect();
        let relevances: Vec<f32> = pool.iter().map(|it| it.score.unwrap_or(0.0)).collect();
        let picks = greedy_pick(&pool_vecs, work_limit, |cand, selected, pool_vecs| {
            lambda * relevances[cand]
                - (1.0 - lambda) * max_cosine_to_selected(cand, selected, pool_vecs)
        });
        let mut taken = vec![false; pool.len()];
        let mut selected: Vec<RecommendResult> = Vec::with_capacity(picks.len());
        for idx in picks {
            taken[idx] = true;
            selected.push(pool[idx].clone());
        }
        let leftover: Vec<RecommendResult> = pool
            .into_iter()
            .enumerate()
            .filter_map(|(i, it)| if taken[i] { None } else { Some(it) })
            .collect();

        [selected, leftover, no_vec, tail_vec].concat()
    }
}
