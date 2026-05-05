use std::sync::Arc;

use qdrant_client::qdrant::{
    point_id::PointIdOptions, vector_output::Vector as VectorVariant,
    vectors_output::VectorsOptions, GetPointsBuilder, PointId, PointStruct, UpsertPointsBuilder,
    Value as QValue,
};
use std::collections::HashMap;
use tracing::debug;

use crate::common::sc_ids::normalize_sc_track_id;
use crate::common::user_id::user_id_to_qdrant_id;
use crate::error::AppResult;
use crate::qdrant::{collections, QdrantService};

const EMA_ALPHA: f32 = 0.25;

fn event_weight(event_type: &str) -> Option<f32> {
    match event_type {
        "like" => Some(1.0),
        "local_like" => Some(1.0),
        "playlist_add" => Some(0.9),
        _ => None,
    }
}

pub struct UserTasteService {
    qdrant: Arc<QdrantService>,
}

impl UserTasteService {
    pub fn new(qdrant: Arc<QdrantService>) -> Arc<Self> {
        Arc::new(Self { qdrant })
    }

    pub async fn on_user_event(
        &self,
        sc_user_id: &str,
        sc_track_id: &str,
        event_type: &str,
    ) -> AppResult<bool> {
        let Some(weight) = event_weight(event_type) else {
            return Ok(false);
        };
        let Some(norm) = normalize_sc_track_id(sc_track_id) else {
            return Ok(false);
        };
        let track_point: u64 = match norm.parse() {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };

        let m = self.update_branch(
            collections::TRACKS_MERT,
            collections::USER_TASTE_MERT,
            track_point,
            sc_user_id,
            weight,
        );
        let c = self.update_branch(
            collections::TRACKS_CLAP,
            collections::USER_TASTE_CLAP,
            track_point,
            sc_user_id,
            weight,
        );
        let l = self.update_branch(
            collections::TRACKS_LYRICS,
            collections::USER_TASTE_LYRICS,
            track_point,
            sc_user_id,
            weight,
        );
        let (rm, rc, rl) = tokio::join!(m, c, l);
        Ok(rm.unwrap_or(false) || rc.unwrap_or(false) || rl.unwrap_or(false))
    }

    async fn update_branch(
        &self,
        track_collection: &str,
        taste_collection: &str,
        track_point_id: u64,
        sc_user_id: &str,
        weight: f32,
    ) -> AppResult<bool> {
        let track_vec = match self.retrieve_vector(track_collection, track_point_id).await {
            Ok(Some(v)) => v,
            Ok(None) => return Ok(false),
            Err(e) => {
                debug!(track_collection, error = %e, "user-taste: track retrieve failed");
                return Ok(false);
            }
        };

        let user_id = user_id_to_qdrant_id(sc_user_id);
        let (current, event_count) = match self.retrieve_taste(taste_collection, user_id).await? {
            Some((vec, ec)) => (Some(vec), ec + 1),
            None => (None, 1u64),
        };

        let mut new_vec = match current {
            Some(curr) if curr.len() == track_vec.len() => curr
                .iter()
                .zip(track_vec.iter())
                .map(|(v, t)| (1.0 - EMA_ALPHA) * v + EMA_ALPHA * weight * t)
                .collect::<Vec<f32>>(),
            _ => track_vec.iter().map(|v| v * weight).collect::<Vec<f32>>(),
        };

        let norm = new_vec
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>()
            .sqrt() as f32;
        if norm > 0.0 {
            for v in &mut new_vec {
                *v /= norm;
            }
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut payload: HashMap<String, QValue> = HashMap::new();
        payload.insert("sc_user_id".into(), QValue::from(sc_user_id.to_string()));
        payload.insert("event_count".into(), QValue::from(event_count as i64));
        payload.insert("updated_at".into(), QValue::from(now_ms));

        let point = PointStruct::new(user_id, new_vec, payload);
        self.qdrant
            .raw()
            .upsert_points(UpsertPointsBuilder::new(taste_collection, vec![point]))
            .await
            .map_err(|e| {
                crate::error::AppError::internal(format!("qdrant upsert {taste_collection}: {e}"))
            })?;
        Ok(true)
    }

    async fn retrieve_vector(&self, collection: &str, id: u64) -> AppResult<Option<Vec<f32>>> {
        let resp = self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(collection, vec![numeric_id(id)]).with_vectors(true))
            .await
            .map_err(|e| {
                crate::error::AppError::internal(format!("qdrant retrieve {collection}: {e}"))
            })?;
        if let Some(point) = resp.result.into_iter().next() {
            if let Some(vectors) = point.vectors {
                if let Some(VectorsOptions::Vector(v)) = vectors.vectors_options {
                    if let VectorVariant::Dense(dense) = v.into_vector() {
                        return Ok(Some(dense.data));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn retrieve_taste(
        &self,
        collection: &str,
        id: u64,
    ) -> AppResult<Option<(Vec<f32>, u64)>> {
        let resp = self
            .qdrant
            .raw()
            .get_points(
                GetPointsBuilder::new(collection, vec![numeric_id(id)])
                    .with_vectors(true)
                    .with_payload(true),
            )
            .await
            .map_err(|e| {
                crate::error::AppError::internal(format!("qdrant retrieve {collection}: {e}"))
            })?;
        let Some(point) = resp.result.into_iter().next() else {
            return Ok(None);
        };
        let vec = match point.vectors.and_then(|v| v.vectors_options) {
            Some(VectorsOptions::Vector(v)) => match v.into_vector() {
                VectorVariant::Dense(dense) => dense.data,
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let event_count = point
            .payload
            .get("event_count")
            .and_then(|v| extract_int(v))
            .unwrap_or(0) as u64;
        Ok(Some((vec, event_count)))
    }
}

pub fn numeric_id(id: u64) -> PointId {
    PointId {
        point_id_options: Some(PointIdOptions::Num(id)),
    }
}

fn extract_int(v: &QValue) -> Option<i64> {
    use qdrant_client::qdrant::value::Kind;
    match &v.kind {
        Some(Kind::IntegerValue(i)) => Some(*i),
        Some(Kind::DoubleValue(d)) => Some(*d as i64),
        _ => None,
    }
}
