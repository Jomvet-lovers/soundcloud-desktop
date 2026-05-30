use qdrant_client::qdrant::{
    point_id::PointIdOptions, vector_output::Vector as VectorVariant,
    vectors_output::VectorsOptions, Filter, GetPointsBuilder, PointId, SearchPointsBuilder,
};
use std::collections::{HashMap, HashSet};
use tracing::debug;

use super::types::RecommendResult;
use super::util::{numeric_id, payload_to_map, point_id_to_value, value_to_u64};
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
        let raw = match self.qdrant.raw().search_points(builder).await {
            Ok(r) => r.result,
            Err(e) => {
                debug!(collection, error = %e, "searchByVector failed");
                return Vec::new();
            }
        };
        // Privacy-guard: Qdrant payload не несёт `sharing`, поэтому отбрасываем
        // приватные треки по source-of-truth (`tracks.sharing`). Иначе любой
        // vector-arm (wave/similar/artist/collab/search) утёк бы private-трек.
        // Дёшево: PK-lookup по ≤limit id; private-точек в индексе единицы.
        let ids: Vec<String> = raw
            .iter()
            .filter_map(|p| value_to_u64(&point_id_to_value(p.id.clone())).map(|n| n.to_string()))
            .collect();
        let public = self.public_track_ids(&ids).await;
        raw.into_iter()
            .filter_map(|p| {
                let id = point_id_to_value(p.id);
                let id_str = value_to_u64(&id)?.to_string();
                if !public.contains(&id_str) {
                    return None;
                }
                Some(RecommendResult {
                    id,
                    score: Some(p.score),
                    payload: Some(payload_to_map(p.payload)),
                    artist: None,
                    genre: None,
                    playback_count: None,
                    features: None,
                })
            })
            .collect()
    }

    /// Подмножество `ids` с `sharing='public'` (source-of-truth privacy-фильтр
    /// для всех vector-arm'ов). Пустой вход / DB-ошибка → пустой набор
    /// (fail-closed: лучше пустой рукав, чем утечка приватного).
    pub(crate) async fn public_track_ids(&self, ids: &[String]) -> HashSet<String> {
        if ids.is_empty() {
            return HashSet::new();
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM tracks WHERE sc_track_id = ANY($1) AND sharing = 'public'",
        )
            .bind(ids)
            .fetch_all(&self.pg)
            .await
            .unwrap_or_default();
        rows.into_iter().map(|(id, )| id).collect()
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
