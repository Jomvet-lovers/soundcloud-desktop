//! Track-arm: рекомендации "от трека" через все 3 audio-коллекции.
//!
//! Сейчас остальные части системы зовут qdrant.recommend только по
//! `tracks_mert` — теряют clap (audio-text) и lyrics. Здесь идём в три
//! коллекции параллельно, для каждой z-нормализуем score внутри окна и
//! складываем с весами `0.5 mert + 0.3 clap + 0.2 lyrics`. Mert как
//! "звуковой каркас" получает основной вес, clap делает выдачу не-monotonic
//! по тембру, lyrics ловит близких по теме.

use std::collections::{HashMap, HashSet};

use qdrant_client::qdrant::{Filter, RecommendPointsBuilder, RecommendStrategy};
use tracing::debug;

use crate::modules::recommendations::service::util::numeric_id;
use crate::modules::recommendations::service::RecommendationsService;
use crate::qdrant::collections;

const MERT_WEIGHT: f32 = 0.5;
const CLAP_WEIGHT: f32 = 0.3;
const LYRICS_WEIGHT: f32 = 0.2;
const PER_COLLECTION: u64 = 30;

#[derive(Debug, Clone)]
pub struct TrackArmCandidate {
    pub sc_track_id: u64,
    /// Смешанный z-score, [-3..+3] в типичном окне.
    pub score: f32,
}

/// Получить рекомендации для одного seed-трека.
pub async fn recommend_from_track(
    svc: &RecommendationsService,
    seed_track_id: u64,
    negative_ids: &[u64],
    filter: Option<&Filter>,
    limit: usize,
) -> Vec<TrackArmCandidate> {
    let (mert, clap, lyrics) = tokio::join!(
        recommend_one(
            svc,
            collections::TRACKS_MERT,
            seed_track_id,
            negative_ids,
            filter
        ),
        recommend_one(
            svc,
            collections::TRACKS_CLAP,
            seed_track_id,
            negative_ids,
            filter
        ),
        recommend_one(
            svc,
            collections::TRACKS_LYRICS,
            seed_track_id,
            negative_ids,
            filter
        ),
    );
    let mut blended: HashMap<u64, f32> = HashMap::new();
    blend_in(&mut blended, &mert, MERT_WEIGHT);
    blend_in(&mut blended, &clap, CLAP_WEIGHT);
    blend_in(&mut blended, &lyrics, LYRICS_WEIGHT);

    let mut out: Vec<TrackArmCandidate> = blended
        .into_iter()
        .map(|(sc_track_id, score)| TrackArmCandidate { sc_track_id, score })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

/// Получить смешанные рекомендации сразу от пачки seed-треков.
/// Свежие seeds (по индексу в массиве — meta-сигнал из signals.rs) получают
/// бóльший вес: используется линейный декей от 1.0 до 0.3.
pub async fn recommend_from_many(
    svc: &RecommendationsService,
    seeds: &[u64],
    negative_ids: &[u64],
    filter: Option<&Filter>,
    limit: usize,
) -> Vec<TrackArmCandidate> {
    if seeds.is_empty() {
        return Vec::new();
    }
    let mut tasks = Vec::with_capacity(seeds.len());
    for (idx, seed) in seeds.iter().copied().enumerate() {
        let recency_weight = recency_factor(idx, seeds.len());
        tasks.push(async move {
            let cands = recommend_from_track(svc, seed, negative_ids, filter, limit * 2).await;
            (cands, recency_weight)
        });
    }
    let per_seed = futures::future::join_all(tasks).await;
    let mut accum: HashMap<u64, f32> = HashMap::new();
    let mut seen_seeds: HashSet<u64> = seeds.iter().copied().collect();
    for (cands, w) in per_seed {
        for c in cands {
            if seen_seeds.contains(&c.sc_track_id) {
                continue;
            }
            let entry = accum.entry(c.sc_track_id).or_insert(0.0);
            *entry += c.score * w;
        }
        seen_seeds.extend(accum.keys());
    }
    let mut out: Vec<TrackArmCandidate> = accum
        .into_iter()
        .map(|(sc_track_id, score)| TrackArmCandidate { sc_track_id, score })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

fn recency_factor(idx: usize, total: usize) -> f32 {
    if total <= 1 {
        return 1.0;
    }
    let t = idx as f32 / (total - 1) as f32;
    (1.0 - 0.7 * t).clamp(0.3, 1.0)
}

async fn recommend_one(
    svc: &RecommendationsService,
    collection: &str,
    seed_track_id: u64,
    negative_ids: &[u64],
    filter: Option<&Filter>,
) -> Vec<(u64, f32)> {
    let mut req = RecommendPointsBuilder::new(collection.to_string(), PER_COLLECTION)
        .with_payload(false)
        .strategy(RecommendStrategy::BestScore)
        .add_positive(numeric_id(seed_track_id));
    for id in negative_ids {
        req = req.add_negative(numeric_id(*id));
    }
    if let Some(f) = filter {
        req = req.filter(f.clone());
    }
    let raw = match svc.qdrant.raw().recommend(req).await {
        Ok(r) => r.result,
        Err(e) => {
            debug!(collection, error = %e, "track_arm: qdrant recommend failed");
            return Vec::new();
        }
    };
    let scored: Vec<(u64, f32)> = raw
        .into_iter()
        .filter_map(|p| {
            use qdrant_client::qdrant::point_id::PointIdOptions;
            let id = p.id?.point_id_options?;
            let n = match id {
                PointIdOptions::Num(n) => n,
                _ => return None,
            };
            if n == seed_track_id {
                return None;
            }
            Some((n, p.score))
        })
        .collect();
    z_normalize(scored)
}

fn z_normalize(mut scored: Vec<(u64, f32)>) -> Vec<(u64, f32)> {
    let n = scored.len();
    if n < 2 {
        return scored.into_iter().map(|(id, _)| (id, 1.0)).collect();
    }
    let mean: f32 = scored.iter().map(|(_, s)| *s).sum::<f32>() / n as f32;
    let var: f32 = scored
        .iter()
        .map(|(_, s)| (*s - mean) * (*s - mean))
        .sum::<f32>()
        / n as f32;
    let std = var.sqrt().max(1e-6);
    for (_, s) in scored.iter_mut() {
        *s = (*s - mean) / std;
    }
    scored
}

fn blend_in(out: &mut HashMap<u64, f32>, src: &[(u64, f32)], weight: f32) {
    for (id, s) in src {
        let entry = out.entry(*id).or_insert(0.0);
        *entry += s * weight;
    }
}
