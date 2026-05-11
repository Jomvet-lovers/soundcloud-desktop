use std::collections::{HashMap, HashSet};

use crate::modules::centroids::cosine;
use crate::qdrant::collections;

use super::mmr::{greedy_pick, max_cosine_to_selected};
use super::service::util::value_to_u64;
use super::service::{RecommendResult, RecommendationsService};

pub struct RerankOptions {
    pub limit: usize,
    pub diversity: f32,
    pub novelty: f32,
    pub serendipity: f32,
    pub anti_centroid: Option<Vec<f32>>,
    pub recent_artists: HashSet<String>,
    pub user_centroid: Option<Vec<f32>>,
}

impl Default for RerankOptions {
    fn default() -> Self {
        Self {
            limit: 12,
            diversity: 0.35,
            novelty: 0.15,
            serendipity: 0.10,
            anti_centroid: None,
            recent_artists: HashSet::new(),
            user_centroid: None,
        }
    }
}

impl RecommendationsService {
    pub async fn rerank_multi(
        &self,
        items: Vec<RecommendResult>,
        opts: RerankOptions,
    ) -> Vec<RecommendResult> {
        if items.is_empty() || opts.limit == 0 {
            return items;
        }
        let work_limit = items.len().min(opts.limit * 4).max(opts.limit);
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
        let vec_map: HashMap<String, Vec<f32>> = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric_ids)
            .await;
        if vec_map.is_empty() {
            return [head_vec, tail_vec].concat();
        }

        let mut pool: Vec<(RecommendResult, Vec<f32>)> = head_vec
            .into_iter()
            .filter_map(|it| {
                let id = value_to_u64(&it.id)?;
                let v = vec_map.get(&id.to_string()).cloned()?;
                Some((it, v))
            })
            .collect();
        if pool.is_empty() {
            return tail_vec;
        }

        if let Some(anti) = opts.anti_centroid.as_deref() {
            pool.retain(|(_, v)| cosine(v, anti) < 0.85);
        }
        pool.sort_by(|a, b| {
            b.0.score
                .unwrap_or(0.0)
                .partial_cmp(&a.0.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if pool.is_empty() {
            return tail_vec;
        }

        let pool_vecs: Vec<Vec<f32>> = pool.iter().map(|(_, v)| v.clone()).collect();
        let relevances: Vec<f32> = pool.iter().map(|(it, _)| it.score.unwrap_or(0.0)).collect();
        let novelty_flags: Vec<f32> = pool
            .iter()
            .map(|(it, _)| match &it.artist {
                Some(a) if opts.recent_artists.contains(&a.to_lowercase()) => 0.0,
                Some(_) => 1.0,
                None => 0.6,
            })
            .collect();
        let user_centroid = opts.user_centroid.clone();

        let picks = greedy_pick(&pool_vecs, opts.limit, |cand, selected, pool_vecs| {
            let rel = relevances[cand];
            let diversity_term = 1.0 - max_cosine_to_selected(cand, selected, pool_vecs);
            let serendipity_term = match user_centroid.as_deref() {
                Some(uc) => (1.0 - cosine(&pool_vecs[cand], uc)).clamp(0.0, 1.0) * rel.max(0.0),
                None => 0.0,
            };
            rel + opts.diversity * diversity_term
                + opts.novelty * novelty_flags[cand]
                + opts.serendipity * serendipity_term
        });

        let mut taken = vec![false; pool.len()];
        let mut selected: Vec<RecommendResult> = Vec::with_capacity(picks.len());
        for idx in picks {
            taken[idx] = true;
            selected.push(pool[idx].0.clone());
        }
        let leftover: Vec<RecommendResult> = pool
            .into_iter()
            .enumerate()
            .filter_map(|(i, (it, _))| if taken[i] { None } else { Some(it) })
            .collect();

        [selected, leftover, tail_vec].concat()
    }
}
