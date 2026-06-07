use deadpool_redis::redis::AsyncCommands;
use qdrant_client::qdrant::{
    point_id::PointIdOptions, vector_output::Vector as VectorVariant,
    vectors_output::VectorsOptions, Filter, GetPointsBuilder, PointId, SearchPointsBuilder,
};
use std::collections::{HashMap, HashSet};
use tracing::debug;

/// Кэш векторов в Redis. Векторы точек ИММУТАБЕЛЬНЫ (эмбеддинг трека не
/// меняется), поэтому кэш безопасен и НЕ влияет на свежесть волны — кэшируем
/// только point-lookup'ы (`retrieve_vectors`), не ANN-поиск. TTL — на случай
/// редкого реиндекса под новой моделью (тогда `qv:`-префикс можно бампнуть).
const VEC_CACHE_TTL: u64 = 6 * 60 * 60;

fn vec_cache_key(collection: &str, id: &str) -> String {
    format!("qv:{collection}:{id}")
}

fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn bytes_to_vec(b: &[u8]) -> Option<Vec<f32>> {
    if b.is_empty() || !b.len().is_multiple_of(4) {
        return None;
    }
    Some(
        b.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

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
        self.retrieve_vectors(collection, &[id])
            .await
            .remove(&id.to_string())
    }

    /// Point-lookup векторов с Redis-кэшем: хиты из кэша (MGET), промахи добираем
    /// из qdrant одним батчем и пишем в кэш (fire-and-forget, не блокируя волну).
    /// Векторы иммутабельны → кэш не влияет на ранжирование/свежесть.
    pub(crate) async fn retrieve_vectors(
        &self,
        collection: &str,
        ids: &[u64],
    ) -> HashMap<String, Vec<f32>> {
        let mut out: HashMap<String, Vec<f32>> = HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let mut uniq: Vec<u64> = ids.to_vec();
        uniq.sort_unstable();
        uniq.dedup();

        // 1) хиты из Redis
        let mut misses: Vec<u64> = Vec::with_capacity(uniq.len());
        match self.redis.get().await {
            Ok(mut conn) => {
                let keys: Vec<String> = uniq
                    .iter()
                    .map(|id| vec_cache_key(collection, &id.to_string()))
                    .collect();
                let cached: Vec<Option<Vec<u8>>> = conn.mget(&keys).await.unwrap_or_default();
                if cached.len() == uniq.len() {
                    for (id, c) in uniq.iter().zip(cached) {
                        match c.as_deref().and_then(bytes_to_vec) {
                            Some(v) => {
                                out.insert(id.to_string(), v);
                            }
                            None => misses.push(*id),
                        }
                    }
                } else {
                    misses = uniq.clone();
                }
            }
            Err(_) => misses = uniq.clone(),
        }
        if misses.is_empty() {
            return out;
        }

        // 2) добор промахов из qdrant одним батчем
        let pids: Vec<PointId> = misses.iter().copied().map(numeric_id).collect();
        let fetched = match self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(collection, pids).with_vectors(true))
            .await
        {
            Ok(r) => r.result,
            Err(e) => {
                debug!(collection, error = %e, "retrieveVectors failed");
                return out;
            }
        };
        let mut to_cache: Vec<(String, Vec<u8>)> = Vec::new();
        for p in fetched {
            let id_str = match p.id.and_then(|id| id.point_id_options) {
                Some(PointIdOptions::Num(n)) => n.to_string(),
                Some(PointIdOptions::Uuid(u)) => u,
                None => continue,
            };
            if let Some(vectors) = p.vectors {
                if let Some(VectorsOptions::Vector(v)) = vectors.vectors_options {
                    if let VectorVariant::Dense(dense) = v.into_vector() {
                        let data = dense.data;
                        to_cache.push((id_str.clone(), vec_to_bytes(&data)));
                        out.insert(id_str, data);
                    }
                }
            }
        }

        // 3) пишем промахи в кэш вне критического пути
        if !to_cache.is_empty() {
            let redis = self.redis.clone();
            let coll = collection.to_string();
            tokio::spawn(async move {
                let Ok(mut conn) = redis.get().await else {
                    return;
                };
                for (id, bytes) in to_cache {
                    let _: Result<(), _> = conn
                        .set_ex::<_, _, ()>(vec_cache_key(&coll, &id), bytes, VEC_CACHE_TTL)
                        .await;
                }
            });
        }
        out
    }
}
