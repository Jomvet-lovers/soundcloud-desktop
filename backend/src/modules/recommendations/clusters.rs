use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::service::RecommendResult;

#[derive(Debug, Serialize)]
pub struct ClusterResponse {
    pub clusters: Vec<Cluster>,
}

#[derive(Debug, Serialize)]
pub struct Cluster {
    pub id: &'static str,
    pub track_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neighbors: Option<Vec<ClusterNeighbor>>,
}

#[derive(Debug, Serialize)]
pub struct ClusterNeighbor {
    pub track_id: String,
    pub artist_id: Uuid,
    pub artist_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

pub struct ClusterBuilder {
    taken: HashSet<String>,
    clusters: Vec<Cluster>,
    features: HashMap<String, Vec<f32>>,
}

impl ClusterBuilder {
    pub fn new() -> Self {
        Self {
            taken: HashSet::new(),
            clusters: Vec::new(),
            features: HashMap::new(),
        }
    }

    pub fn attach_features(&mut self, track_id: String, features: Vec<f32>) {
        self.features.insert(track_id, features);
    }

    pub fn features_map(&self) -> &HashMap<String, Vec<f32>> {
        &self.features
    }

    pub fn reserve(&mut self, ids: impl IntoIterator<Item = String>) {
        for id in ids {
            self.taken.insert(id);
        }
    }

    pub fn taken(&self) -> &HashSet<String> {
        &self.taken
    }

    pub fn push(&mut self, id: &'static str, track_ids: Vec<String>) {
        if track_ids.is_empty() {
            return;
        }
        for t in &track_ids {
            self.taken.insert(t.clone());
        }
        self.clusters.push(Cluster {
            id,
            track_ids,
            neighbors: None,
        });
    }

    pub fn push_with_neighbors(
        &mut self,
        id: &'static str,
        neighbors: Vec<ClusterNeighbor>,
    ) {
        if neighbors.is_empty() {
            return;
        }
        let track_ids: Vec<String> = neighbors.iter().map(|n| n.track_id.clone()).collect();
        for t in &track_ids {
            self.taken.insert(t.clone());
        }
        self.clusters.push(Cluster {
            id,
            track_ids,
            neighbors: Some(neighbors),
        });
    }

    pub fn finish(self) -> ClusterResponse {
        ClusterResponse {
            clusters: self.clusters,
        }
    }

    pub fn all_track_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in &self.clusters {
            for id in &c.track_ids {
                out.push(id.clone());
            }
        }
        out
    }

    pub fn drop_missing(&mut self, missing: &HashSet<String>) {
        if missing.is_empty() {
            return;
        }
        let mut kept: Vec<Cluster> = Vec::with_capacity(self.clusters.len());
        for c in self.clusters.drain(..) {
            let track_ids: Vec<String> = c
                .track_ids
                .into_iter()
                .filter(|id| !missing.contains(id))
                .collect();
            if track_ids.is_empty() {
                continue;
            }
            let neighbors = c.neighbors.map(|ns| {
                ns.into_iter()
                    .filter(|n| !missing.contains(&n.track_id))
                    .collect::<Vec<_>>()
            });
            kept.push(Cluster {
                id: c.id,
                track_ids,
                neighbors: neighbors.and_then(|ns| if ns.is_empty() { None } else { Some(ns) }),
            });
        }
        self.clusters = kept;
        for id in missing.iter() {
            self.features.remove(id);
        }
    }
}

impl Default for ClusterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn recommend_id_str(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(n) = v.as_u64() {
        return n.to_string();
    }
    String::new()
}

pub fn pick_unique_ids(
    pool: &[RecommendResult],
    taken: &HashSet<String>,
    limit: usize,
) -> Vec<String> {
    let mut out = Vec::with_capacity(limit);
    for it in pool {
        if out.len() >= limit {
            break;
        }
        let id = recommend_id_str(&it.id);
        if id.is_empty() || taken.contains(&id) {
            continue;
        }
        out.push(id);
    }
    out
}

