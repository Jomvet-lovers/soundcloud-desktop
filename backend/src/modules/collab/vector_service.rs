use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use qdrant_client::qdrant::{
    point_id::PointIdOptions, vector_output::Vector as VectorVariant,
    vectors_output::VectorsOptions, GetCollectionInfoRequest, GetPointsBuilder, PointId,
};
use sqlx::PgPool;
use tracing::debug;

use crate::error::AppResult;
use crate::qdrant::{collections, QdrantService};

const TTL: Duration = Duration::from_secs(5 * 60);
const DIM_RECHECK: Duration = Duration::from_secs(60);
const MAX_LIKES: i64 = 50;

const POSITIVE_TYPES: &[&str] = &["like", "local_like", "playlist_add"];

#[derive(Debug, Clone)]
struct CacheEntry {
    vector: Option<Vec<f32>>,
    expires_at: Instant,
}

pub struct CollabVectorService {
    qdrant: Arc<QdrantService>,
    pg: PgPool,
    cache: RwLock<HashMap<String, CacheEntry>>,
    dim: RwLock<DimCache>,
}

#[derive(Debug, Clone, Default)]
struct DimCache {
    dim: Option<u32>,
    checked_at: Option<Instant>,
}

impl CollabVectorService {
    pub fn new(qdrant: Arc<QdrantService>, pg: PgPool) -> Arc<Self> {
        Arc::new(Self {
            qdrant,
            pg,
            cache: RwLock::new(HashMap::new()),
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

    pub async fn get_track_vectors(&self, ids: &[u64]) -> HashMap<String, Vec<f32>> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let pids: Vec<PointId> = ids.iter().copied().map(numeric_id).collect();
        let resp = match self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(collections::TRACKS_COLLAB, pids).with_vectors(true))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "getTrackVectors failed");
                return out;
            }
        };
        for p in resp.result {
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
        out
    }

    pub async fn get_user_vector(&self, sc_user_id: &str) -> AppResult<Option<Vec<f32>>> {
        if let Ok(g) = self.cache.read() {
            if let Some(entry) = g.get(sc_user_id) {
                if entry.expires_at > Instant::now() {
                    return Ok(entry.vector.clone());
                }
            }
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM user_events \
             WHERE sc_user_id = $1 AND event_type = ANY($2) \
             ORDER BY created_at DESC LIMIT $3",
        )
        .bind(sc_user_id)
        .bind(POSITIVE_TYPES)
        .bind(MAX_LIKES)
        .fetch_all(&self.pg)
        .await?;
        if rows.is_empty() {
            self.put_cache(sc_user_id, None);
            return Ok(None);
        }
        let ids: Vec<u64> = rows
            .iter()
            .filter_map(|(id,)| id.parse::<u64>().ok())
            .collect();
        let vecs = self.get_track_vectors(&ids).await;
        if vecs.is_empty() {
            self.put_cache(sc_user_id, None);
            return Ok(None);
        }
        let dim = vecs.values().next().map(|v| v.len()).unwrap_or(0);
        if dim == 0 {
            self.put_cache(sc_user_id, None);
            return Ok(None);
        }
        let mut acc = vec![0f64; dim];
        for v in vecs.values() {
            if v.len() != dim {
                continue;
            }
            for (i, x) in v.iter().enumerate() {
                acc[i] += *x as f64;
            }
        }
        let n = vecs.len() as f64;
        let mut mean: Vec<f32> = acc.into_iter().map(|x| (x / n) as f32).collect();
        crate::modules::centroids::normalize(&mut mean);
        self.put_cache(sc_user_id, Some(mean.clone()));
        Ok(Some(mean))
    }

    fn put_cache(&self, sc_user_id: &str, vector: Option<Vec<f32>>) {
        if let Ok(mut g) = self.cache.write() {
            g.insert(
                sc_user_id.to_string(),
                CacheEntry {
                    vector,
                    expires_at: Instant::now() + TTL,
                },
            );
        }
    }

    pub fn invalidate(&self, sc_user_id: &str) {
        if let Ok(mut g) = self.cache.write() {
            g.remove(sc_user_id);
        }
    }

    pub fn invalidate_all(&self) {
        if let Ok(mut g) = self.cache.write() {
            g.clear();
        }
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
