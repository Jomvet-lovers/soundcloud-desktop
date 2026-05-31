use std::sync::Arc;

use qdrant_client::config::QdrantConfig;
use qdrant_client::qdrant::{
    vector_output::Vector as VectorVariant, vectors_output::VectorsOptions,
    CreateCollectionBuilder, Distance, GetCollectionInfoRequest, GetPointsBuilder,
    HnswConfigDiffBuilder, PointId, PointStruct, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use serde_json::json;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::config::QdrantCfg;
use crate::error::{AppError, AppResult};

pub mod collections {
    pub const TRACKS_MERT: &str = "tracks_mert";
    pub const TRACKS_CLAP: &str = "tracks_clap";
    pub const TRACKS_LYRICS: &str = "tracks_lyrics";
    pub const TRACKS_COLLAB: &str = "tracks_collab";

    /// Durable-кэш векторов текстовых запросов (vibe MuLan / lyrics bge-m3).
    /// Используется как KV: точка по UUID(sha256(query)), get-by-id, без ANN —
    /// поэтому коллекции создаются с `hnsw m=0` (граф не строится) и векторами
    /// on_disk. Размерности зеркалят `tracks_clap`/`tracks_lyrics`.
    pub const QUERY_VEC_MULAN: &str = "query_vectors_mulan";
    pub const QUERY_VEC_LYRICS: &str = "query_vectors_lyrics";
}

pub struct QdrantService {
    client: Qdrant,
}

impl QdrantService {
    pub fn connect(cfg: &QdrantCfg) -> AppResult<Arc<Self>> {
        let mut qcfg = QdrantConfig::from_url(&cfg.url);
        if !cfg.api_key.is_empty() {
            qcfg = qcfg.api_key(cfg.api_key.clone());
        }
        let client = Qdrant::new(qcfg)
            .map_err(|e| AppError::internal(format!("qdrant client init: {e}")))?;
        Ok(Arc::new(Self { client }))
    }

    pub fn raw(&self) -> &Qdrant {
        &self.client
    }

    pub async fn bootstrap_collections(&self) {
        let collections = match self.client.list_collections().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "Qdrant init skipped (not available)");
                return;
            }
        };
        let existing: std::collections::HashSet<String> = collections
            .collections
            .into_iter()
            .map(|c| c.name)
            .collect();

        for (name, size) in [
            (collections::TRACKS_MERT, 1024u64),
            (collections::TRACKS_CLAP, 512),
            (collections::TRACKS_LYRICS, 1024),
        ] {
            if existing.contains(name) {
                continue;
            }
            let req = qdrant_client::qdrant::CreateCollectionBuilder::new(name)
                .vectors_config(VectorParamsBuilder::new(size, Distance::Cosine))
                .on_disk_payload(true)
                .build();
            match self.client.create_collection(req).await {
                Ok(_) => info!(collection = name, size, "Qdrant collection created"),
                Err(e) => warn!(collection = name, error = %e, "Qdrant collection create failed"),
            }
        }

        // Query-vec коллекции: KV по UUID(hash), только get-by-id → HNSW не нужен
        // (m=0, граф не строится, ноль оверхеда на upsert), вектора on_disk.
        for (name, size) in [
            (collections::QUERY_VEC_MULAN, 512u64),
            (collections::QUERY_VEC_LYRICS, 1024),
        ] {
            if existing.contains(name) {
                continue;
            }
            let req = CreateCollectionBuilder::new(name)
                .vectors_config(VectorParamsBuilder::new(size, Distance::Cosine).on_disk(true))
                .hnsw_config(HnswConfigDiffBuilder::default().m(0))
                .on_disk_payload(true)
                .build();
            match self.client.create_collection(req).await {
                Ok(_) => info!(
                    collection = name,
                    size, "Qdrant query-vec collection created (hnsw m=0)"
                ),
                Err(e) => {
                    warn!(collection = name, error = %e, "Qdrant query-vec collection create failed")
                }
            }
        }
    }

    /// Durable-запись вектора запроса: point id = детерминированный UUID из
    /// sha256-хэша запроса (тот же хэш, что Redis-ключ). Пустые не пишем (вызов
    /// гейтит звонящий) — durable-стор хранит только реальные вектора.
    pub async fn upsert_query_vector(
        &self,
        collection: &str,
        hash: &str,
        vec: Vec<f32>,
    ) -> AppResult<()> {
        let payload = Payload::from(json_map([(
            "created_at",
            json!(chrono::Utc::now().timestamp()),
        )]));
        self.client
            .upsert_points(UpsertPointsBuilder::new(
                collection,
                vec![PointStruct::new(query_point_id(hash), vec, payload)],
            ))
            .await
            .map_err(|e| AppError::internal(format!("qdrant upsert query vec: {e}")))?;
        Ok(())
    }

    /// Get-by-id (НЕ ANN) вектора запроса из durable-стора. None — нет точки или
    /// Qdrant недоступен (звонящий упадёт на Redis/воркер).
    pub async fn get_query_vector(&self, collection: &str, hash: &str) -> Option<Vec<f32>> {
        let resp = self
            .client
            .get_points(
                GetPointsBuilder::new(collection, vec![query_point_id(hash)]).with_vectors(true),
            )
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

    /// Все vector-write'ы живут здесь: воркер считает эмбеддинги и шлёт их в
    /// NATS, в Qdrant пишет только backend (см. AGENTS.md воркера). Point id =
    /// числовой sc_track_id; payload.sc_track_id (строка) используется как
    /// filter-ключ в recommendations.
    pub async fn upsert_audio(
        &self,
        sc_track_id: u64,
        mert: Vec<f32>,
        clap: Vec<f32>,
        language: Option<&str>,
    ) -> AppResult<()> {
        let payload = track_payload(sc_track_id, "indexed_at", language);
        self.client
            .upsert_points(UpsertPointsBuilder::new(
                collections::TRACKS_MERT,
                vec![PointStruct::new(sc_track_id, mert, payload.clone())],
            ))
            .await
            .map_err(|e| AppError::internal(format!("qdrant upsert mert: {e}")))?;
        self.client
            .upsert_points(UpsertPointsBuilder::new(
                collections::TRACKS_CLAP,
                vec![PointStruct::new(sc_track_id, clap, payload)],
            ))
            .await
            .map_err(|e| AppError::internal(format!("qdrant upsert clap: {e}")))?;
        Ok(())
    }

    pub async fn upsert_lyrics(
        &self,
        sc_track_id: u64,
        vec: Vec<f32>,
        language: Option<&str>,
    ) -> AppResult<()> {
        let payload = track_payload(sc_track_id, "embedded_at", language);
        self.client
            .upsert_points(UpsertPointsBuilder::new(
                collections::TRACKS_LYRICS,
                vec![PointStruct::new(sc_track_id, vec, payload)],
            ))
            .await
            .map_err(|e| AppError::internal(format!("qdrant upsert lyrics: {e}")))?;
        Ok(())
    }

    pub async fn upsert_collab(&self, dim: u64, points: Vec<(u64, Vec<f32>)>) -> AppResult<usize> {
        if points.is_empty() {
            return Ok(0);
        }
        self.ensure_collab_collection(dim).await?;
        let structs: Vec<PointStruct> = points
            .into_iter()
            .map(|(id, v)| {
                PointStruct::new(
                    id,
                    v,
                    Payload::from(json_map([("sc_track_id", json!(id.to_string()))])),
                )
            })
            .collect();
        let total = structs.len();
        self.client
            .upsert_points_chunked(
                UpsertPointsBuilder::new(collections::TRACKS_COLLAB, structs),
                500,
            )
            .await
            .map_err(|e| AppError::internal(format!("qdrant upsert collab: {e}")))?;
        Ok(total)
    }

    /// collab-вектор имеет динамическую размерность (зависит от тренировки),
    /// поэтому коллекция создаётся лениво на первом upsert'е, а при смене dim —
    /// пересоздаётся (старые вектора несовместимы).
    async fn ensure_collab_collection(&self, dim: u64) -> AppResult<()> {
        let current = self
            .client
            .collection_info(GetCollectionInfoRequest {
                collection_name: collections::TRACKS_COLLAB.into(),
            })
            .await
            .ok()
            .and_then(|r| r.result)
            .and_then(|c| c.config)
            .and_then(|c| c.params)
            .and_then(|p| p.vectors_config)
            .and_then(|vc| match vc.config {
                Some(qdrant_client::qdrant::vectors_config::Config::Params(p)) => Some(p.size),
                _ => None,
            });
        match current {
            Some(d) if d == dim => return Ok(()),
            Some(d) => {
                warn!(
                    got = d,
                    want = dim,
                    "collab collection dim mismatch — recreating"
                );
                let _ = self
                    .client
                    .delete_collection(collections::TRACKS_COLLAB)
                    .await;
            }
            None => {}
        }
        let req = CreateCollectionBuilder::new(collections::TRACKS_COLLAB)
            .vectors_config(VectorParamsBuilder::new(dim, Distance::Cosine))
            .on_disk_payload(true)
            .build();
        self.client
            .create_collection(req)
            .await
            .map_err(|e| AppError::internal(format!("collab collection create: {e}")))?;
        info!(dim, "collab collection created");
        Ok(())
    }
}

/// JSON-массив чисел → `Vec<f32>`. None если поля нет, оно не массив, пусто или
/// содержит не-finite/не-числа (частичный или NaN/Inf вектор в Qdrant нельзя).
pub fn parse_f32_vec(v: Option<&serde_json::Value>) -> Option<Vec<f32>> {
    let arr = v?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let out: Vec<f32> = arr
        .iter()
        .filter_map(|x| x.as_f64().filter(|f| f.is_finite()).map(|f| f as f32))
        .collect();
    (out.len() == arr.len()).then_some(out)
}

fn json_map<const N: usize>(
    entries: [(&str, serde_json::Value); N],
) -> serde_json::Map<String, serde_json::Value> {
    entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// sha256-hex (64 ASCII chars) → детерминированный UUID-string point id (Qdrant
/// принимает только u64/UUID). Берём первые 16 байт (32 hex) хэша. Кривой/короткий
/// `hash` (битый `done.encode` payload) не должен паниковать слайсом — для него
/// берём детерминированный sha256-фолбэк (upsert и get согласованы, т.к. функция
/// чистая).
fn query_point_id(hash: &str) -> PointId {
    let valid32 = hash.len() >= 32 && hash.as_bytes()[..32].iter().all(u8::is_ascii_hexdigit);
    let fallback;
    let h: &str = if valid32 {
        &hash[..32]
    } else {
        fallback = hex::encode(Sha256::digest(hash.as_bytes()));
        &fallback[..32]
    };
    let uuid = format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    );
    PointId::from(uuid)
}

fn track_payload(sc_track_id: u64, ts_key: &str, language: Option<&str>) -> Payload {
    let mut m = json_map([
        ("sc_track_id", json!(sc_track_id.to_string())),
        (ts_key, json!(chrono::Utc::now().timestamp())),
    ]);
    if let Some(l) = language.filter(|l| !l.is_empty()) {
        m.insert("language".to_string(), json!(l));
    }
    Payload::from(m)
}
