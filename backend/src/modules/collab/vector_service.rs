use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use qdrant_client::qdrant::{
    point_id::PointIdOptions, vector_output::Vector as VectorVariant,
    vectors_output::VectorsOptions, GetCollectionInfoRequest, GetPointsBuilder, PointId,
};

use crate::qdrant::{collections, QdrantService};

const DIM_RECHECK: Duration = Duration::from_secs(60);

pub struct CollabVectorService {
    qdrant: Arc<QdrantService>,
    dim: RwLock<DimCache>,
}

#[derive(Debug, Clone, Default)]
struct DimCache {
    dim: Option<u32>,
    checked_at: Option<Instant>,
}

impl CollabVectorService {
    pub fn new(qdrant: Arc<QdrantService>) -> Arc<Self> {
        Arc::new(Self {
            qdrant,
            dim: RwLock::new(DimCache::default()),
        })
    }

    pub async fn get_collab_dim(&self) -> Option<u32> {
        {
            let g = self.dim.read().ok()?;
            if let Some(checked) = g.checked_at {
                if checked.elapsed() < DIM_RECHECK {
                    return g.dim;
                }
            }
        }
        self.detect_collab_dim().await
    }

    async fn detect_collab_dim(&self) -> Option<u32> {
        let info = self
            .qdrant
            .raw()
            .collection_info(GetCollectionInfoRequest {
                collection_name: collections::TRACKS_COLLAB.into(),
            })
            .await
            .ok()
            .and_then(|r| r.result);
        let dim = info
            .and_then(|c| c.config)
            .and_then(|c| c.params)
            .and_then(|p| p.vectors_config)
            .and_then(|vc| match vc.config {
                Some(qdrant_client::qdrant::vectors_config::Config::Params(p)) => {
                    Some(p.size as u32)
                }
                _ => None,
            });
        if let Ok(mut g) = self.dim.write() {
            g.dim = dim;
            g.checked_at = Some(Instant::now());
        }
        dim
    }

    pub async fn get_track_vector(&self, sc_track_id: u64) -> Option<Vec<f32>> {
        let resp = self
            .qdrant
            .raw()
            .get_points(
                GetPointsBuilder::new(collections::TRACKS_COLLAB, vec![numeric_id(sc_track_id)])
                    .with_vectors(true),
            )
            .await
            .ok()?;
        let point = resp.result.first()?;
        let vectors = point.vectors.as_ref()?;
        match vectors.vectors_options.clone() {
            Some(VectorsOptions::Vector(v)) => match v.into_vector() {
                VectorVariant::Dense(dense) => Some(dense.data),
                _ => None,
            },
            _ => None,
        }
    }

    /// Сбросить кэш размерности коллаб-коллекции (после переобучения/реиндекса).
    pub fn invalidate_all(&self) {
        if let Ok(mut g) = self.dim.write() {
            *g = DimCache::default();
        }
    }
}

pub fn numeric_id(id: u64) -> PointId {
    PointId {
        point_id_options: Some(PointIdOptions::Num(id)),
    }
}
