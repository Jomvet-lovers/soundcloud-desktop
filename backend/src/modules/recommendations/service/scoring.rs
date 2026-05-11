use std::collections::HashMap;
use tracing::info;

use crate::modules::centroids::cosine;
use crate::modules::ltr::LTR_FEATURE_COUNT;
use crate::qdrant::collections;

use super::types::{ScoredCandidate, SeedVectors};
use super::RecommendationsService;

impl RecommendationsService {
    pub(crate) async fn score_by_all_bases(
        &self,
        candidate_ids: &[u64],
        seed: &SeedVectors,
        req_id: &str,
    ) -> Vec<ScoredCandidate> {
        let collab_fut = async {
            if seed.collab.is_some() {
                self.collab.get_track_vectors(candidate_ids).await
            } else {
                HashMap::new()
            }
        };
        let mert_fut = async {
            if seed.mert.is_some() {
                self.retrieve_vectors(collections::TRACKS_MERT, candidate_ids)
                    .await
            } else {
                HashMap::new()
            }
        };
        let clap_fut = async {
            if seed.clap.is_some() {
                self.retrieve_vectors(collections::TRACKS_CLAP, candidate_ids)
                    .await
            } else {
                HashMap::new()
            }
        };
        let lyrics_fut = async {
            if seed.lyrics.is_some() {
                self.retrieve_vectors(collections::TRACKS_LYRICS, candidate_ids)
                    .await
            } else {
                HashMap::new()
            }
        };
        let payload_fut = async {
            self.retrieve_payloads(collections::TRACKS_MERT, candidate_ids)
                .await
        };

        let (collab_map, mert_map, clap_map, lyrics_map, payload_map) =
            tokio::join!(collab_fut, mert_fut, clap_fut, lyrics_fut, payload_fut);

        let c_mert = self.centroids.get(collections::TRACKS_MERT);
        let c_clap = self.centroids.get(collections::TRACKS_CLAP);
        let w_col = self.cfg.collab_weight as f32;
        let w_m = self.cfg.audio_weight as f32;
        let w_c = self.cfg.clap_weight as f32;
        let w_l = self.cfg.lyrics_weight as f32;

        let mut with_collab = 0usize;
        let mut out: Vec<ScoredCandidate> = Vec::with_capacity(candidate_ids.len());
        for id in candidate_ids {
            let key = id.to_string();
            let tcol = collab_map.get(&key);
            let tm = mert_map.get(&key);
            let tc = clap_map.get(&key);
            let tl = lyrics_map.get(&key);
            let s_col = match (&seed.collab, tcol) {
                (Some(s), Some(t)) => cosine(t, s),
                _ => 0.0,
            };
            let s_m = match (&seed.mert, tm) {
                (Some(s), Some(t)) => self.centroids.whitened_cosine(t, s, c_mert.as_deref()),
                _ => 0.0,
            };
            let s_c = match (&seed.clap, tc) {
                (Some(s), Some(t)) => self.centroids.whitened_cosine(t, s, c_clap.as_deref()),
                _ => 0.0,
            };
            let s_l = match (&seed.lyrics, tl) {
                (Some(s), Some(t)) => cosine(t, s),
                _ => 0.0,
            };
            if tcol.is_some() {
                with_collab += 1;
            }
            let score = w_col * s_col + w_m * s_m + w_c * s_c + w_l * s_l;
            let mut features = vec![0f32; LTR_FEATURE_COUNT];
            features[0] = s_col;
            features[1] = s_m;
            features[2] = s_c;
            features[3] = s_l;
            out.push(ScoredCandidate {
                id: *id,
                score,
                payload: payload_map.get(&key).cloned(),
                features,
            });
        }
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        info!(req_id, total = out.len(), with_collab, "scored");
        out
    }
}
