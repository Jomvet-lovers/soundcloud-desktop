use qdrant_client::qdrant::{
    point_id::PointIdOptions, vector_output::Vector as VectorVariant,
    vectors_output::VectorsOptions, Filter, GetPointsBuilder, PointId, SearchPointsBuilder,
};
use std::collections::HashMap;
use tracing::debug;

use super::types::RecommendResult;
use super::util::{numeric_id, payload_to_map, point_id_to_value};
use super::RecommendationsService;

impl RecommendationsService {
    pub(crate) async fn search_by_vector(
        &self,
        collection: &str,
        vector: &[f32],
        filter: Option<&Filter>,
        limit: usize,
    ) -> Vec<RecommendResult> {
        let mut builder =
            SearchPointsBuilder::new(collection, vector.to_vec(), limit as u64).with_payload(true);
        if let Some(f) = filter {
            builder = builder.filter(f.clone());
        }
        match self.qdrant.raw().search_points(builder).await {
            Ok(r) => r
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
                .collect(),
            Err(e) => {
                debug!(collection, error = %e, "searchByVector failed");
                Vec::new()
            }
        }
    }

    pub(crate) async fn retrieve_vector(&self, collection: &str, id: u64) -> Option<Vec<f32>> {
        let resp = self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(collection, vec![numeric_id(id)]).with_vectors(true))
            .await
            .ok()?;
        let p = resp.result.into_iter().next()?;
        match p.vectors.and_then(|v| v.vectors_options)? {
            VectorsOptions::Vector(v) => match v.into_vector() {
                VectorVariant::Dense(dense) => Some(dense.data),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) async fn retrieve_vectors(
        &self,
        collection: &str,
        ids: &[u64],
    ) -> HashMap<String, Vec<f32>> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let pids: Vec<PointId> = ids.iter().copied().map(numeric_id).collect();
        match self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(collection, pids).with_vectors(true))
            .await
        {
            Ok(r) => {
                for p in r.result {
                    let id_str = match p.id.and_then(|id| id.point_id_options) {
                        Some(PointIdOptions::Num(n)) => n.to_string(),
                        Some(PointIdOptions::Uuid(u)) => u,
                        None => continue,
                    };
                    if let Some(vectors) = p.vectors {
                        if let Some(VectorsOptions::Vector(v)) = vectors.vectors_options {
                            if let VectorVariant::Dense(dense) = v.into_vector() {
                                out.insert(id_str, dense.data);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                debug!(collection, error = %e, "retrieveVectors failed");
            }
        }
        out
    }

}
