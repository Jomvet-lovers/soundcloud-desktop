use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use qdrant_client::qdrant::{
    vector_output::Vector as VectorVariant, vectors_output::VectorsOptions, ScrollPointsBuilder,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::error::AppResult;
use crate::qdrant::{collections, QdrantService};

const SAMPLE_SIZE: usize = 2000;
const REFRESH: Duration = Duration::from_secs(60 * 60);
const SCROLL_BATCH: u32 = 256;

#[derive(Debug, Clone)]
struct CentroidEntry {
    vector: Option<Vec<f32>>,
}

pub struct CentroidService {
    qdrant: Arc<QdrantService>,
    cache: RwLock<HashMap<String, CentroidEntry>>,
}

impl CentroidService {
    pub fn new(qdrant: Arc<QdrantService>) -> Arc<Self> {
        Arc::new(Self {
            qdrant,
            cache: RwLock::new(HashMap::new()),
        })
    }

    pub fn get(&self, collection: &str) -> Option<Vec<f32>> {
        let guard = self.cache.read().ok()?;
        guard.get(collection).and_then(|e| e.vector.clone())
    }

    pub fn whitened_cosine(&self, a: &[f32], b: &[f32], centroid: Option<&[f32]>) -> f32 {
        match centroid {
            None => cosine(a, b),
            Some(c) => {
                let aw = subtract(a, c);
                let bw = subtract(b, c);
                cosine(&aw, &bw)
            }
        }
    }

    pub async fn refresh_all(&self) {
        let m = self.refresh(collections::TRACKS_MERT);
        let c = self.refresh(collections::TRACKS_CLAP);
        let _ = tokio::join!(m, c);
    }

    pub fn spawn_refresh_loop(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        tokio::spawn(async move {
            svc.refresh_all().await;
            let mut ticker = tokio::time::interval(REFRESH);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => svc.refresh_all().await,
                }
            }
        });
    }

    async fn refresh(&self, collection: &str) {
        match self.refresh_inner(collection).await {
            Ok((count, dim)) if count == 0 => {
                if let Ok(mut g) = self.cache.write() {
                    g.insert(collection.to_string(), CentroidEntry { vector: None });
                }
                warn!(collection, "centroid: empty collection");
            }
            Ok((count, dim)) => {
                info!(collection, count, dim, "centroid refreshed");
            }
            Err(e) => {
                warn!(collection, error = %e, "centroid refresh failed");
            }
        }
    }

    async fn refresh_inner(&self, collection: &str) -> AppResult<(usize, usize)> {
        let mut acc: Vec<f64> = Vec::new();
        let mut count: usize = 0;
        let mut offset: Option<qdrant_client::qdrant::PointId> = None;

        loop {
            let remaining = SAMPLE_SIZE.saturating_sub(count);
            if remaining == 0 {
                break;
            }
            let limit = (SCROLL_BATCH as usize).min(remaining) as u32;
            let mut builder = ScrollPointsBuilder::new(collection)
                .limit(limit)
                .with_vectors(true)
                .with_payload(false);
            if let Some(off) = offset.clone() {
                builder = builder.offset(off);
            }
            let resp = self.qdrant.raw().scroll(builder).await.map_err(|e| {
                crate::error::AppError::internal(format!("qdrant scroll {collection}: {e}"))
            })?;

            if resp.result.is_empty() {
                break;
            }

            for point in &resp.result {
                let Some(vectors) = &point.vectors else {
                    continue;
                };
                let Some(VectorsOptions::Vector(v)) = vectors.vectors_options.clone() else {
                    continue;
                };
                let VectorVariant::Dense(dense) = v.into_vector() else {
                    continue;
                };
                if dense.data.is_empty() {
                    continue;
                }
                if acc.is_empty() {
                    acc = vec![0.0; dense.data.len()];
                }
                if acc.len() != dense.data.len() {
                    continue;
                }
                for (i, val) in dense.data.iter().enumerate() {
                    acc[i] += *val as f64;
                }
                count += 1;
            }

            offset = resp.next_page_offset;
            if offset.is_none() || count >= SAMPLE_SIZE {
                break;
            }
        }

        if count == 0 {
            return Ok((0, 0));
        }

        let mut mean: Vec<f32> = acc.iter().map(|v| (v / count as f64) as f32).collect();
        let norm = mean
            .iter()
            .map(|x| (*x as f64) * (*x as f64))
            .sum::<f64>()
            .sqrt() as f32;
        if norm > 0.0 {
            for v in &mut mean {
                *v /= norm;
            }
        }
        let dim = mean.len();
        if let Ok(mut g) = self.cache.write() {
            g.insert(collection.to_string(), CentroidEntry { vector: Some(mean) });
        }
        Ok((count, dim))
    }
}

fn subtract(a: &[f32], b: &[f32]) -> Vec<f32> {
    let n = a.len().min(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(a[i] - b[i]);
    }
    out
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0f64;
    let mut na = 0f64;
    let mut nb = 0f64;
    for i in 0..n {
        dot += (a[i] as f64) * (b[i] as f64);
        na += (a[i] as f64) * (a[i] as f64);
        nb += (b[i] as f64) * (b[i] as f64);
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom > 0.0 {
        (dot / denom) as f32
    } else {
        0.0
    }
}

pub fn normalize(v: &mut [f32]) {
    let norm = v
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt() as f32;
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
}
