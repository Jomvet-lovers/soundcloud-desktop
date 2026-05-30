use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::bus::nats::NatsService;
use crate::bus::subjects::{self, streams};
use crate::cache::cache_service::CacheScope;
use crate::cache::CacheService;
use crate::error::AppResult;
use crate::qdrant::{collections, parse_f32_vec, QdrantService};

/// Hit: вектор детерминирован для модели → держим долго, повтор никогда не
/// трогает воркер. `v1` бампается при смене модели.
const VEC_CACHE_TTL_SECS: u64 = 30 * 24 * 60 * 60;
/// Negative (воркер реально вернул пусто / ошибка): коротко, чтобы временно
/// дохлый воркер или ещё-не-проиндексированный корпус переспросились скоро,
/// а не залипли на месяц.
const VEC_NEG_TTL_SECS: u64 = 2 * 60 * 60;
/// In-flight маркер: пока джоб считается воркером, повтор того же запроса не
/// плодит новый джоб (single-flight на публикацию). Истекает сам, если воркер
/// умер не ответив — тогда следующий запрос пере-диспатчит.
const ENCODE_INFLIGHT_TTL_SECS: u64 = 15 * 60;
/// Гибрид-бюджет ожидания результата в запросе: ~10s (20 × 500ms). Воркер
/// свободен → вектор успевает прилететь, отдаём сразу; занят → возвращаем
/// Preparing, а джоб дольёт кэш в фоне.
const ENCODE_POLL_TRIES: usize = 20;
const ENCODE_POLL_INTERVAL_MS: u64 = 500;

/// Исход энкода для read-path: готов / пусто (воркер без вектора) / ещё
/// считается (Preparing — фронту показать «готовим вайб», переспросить).
#[derive(Debug, Clone)]
pub enum EncodeOutcome {
    Ready(Vec<f32>),
    Empty,
    Preparing,
}

/// Описатель модели энкода: имя для джоба, Redis-префикс, durable-коллекция.
struct EncodeModel {
    model: &'static str,
    prefix: &'static str,
    collection: &'static str,
}

const MULAN: EncodeModel = EncodeModel {
    model: "mulan",
    prefix: "vibe:vec:mulan:v1:",
    collection: collections::QUERY_VEC_MULAN,
};
const LYRICS: EncodeModel = EncodeModel {
    model: "lyrics",
    prefix: "vibe:vec:lyrics:v1:",
    collection: collections::QUERY_VEC_LYRICS,
};

#[derive(Debug, Clone, Serialize)]
pub struct RankCandidate {
    pub idx: usize,
    pub source: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RankResult {
    pub best_idx: usize,
    pub score: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LangResult {
    pub language: String,
    pub confidence: f32,
}

pub struct WorkerClient {
    nats: Arc<NatsService>,
    cache: Arc<CacheService>,
    qdrant: Arc<QdrantService>,
}

impl WorkerClient {
    pub fn new(
        nats: Arc<NatsService>,
        cache: Arc<CacheService>,
        qdrant: Arc<QdrantService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            nats,
            cache,
            qdrant,
        })
    }

    pub async fn detect_language(&self, text: &str) -> AppResult<Option<LangResult>> {
        self.nats
            .request::<_, LangResult>(
                subjects::AI_DETECT_LANGUAGE,
                &serde_json::json!({ "text": text }),
                Duration::from_secs(15),
                false,
            )
            .await
    }

    pub async fn generate_search_queries(
        &self,
        artist: &str,
        title: &str,
    ) -> AppResult<Vec<String>> {
        #[derive(Deserialize)]
        struct Resp {
            queries: Option<Vec<String>>,
        }
        let res: Option<Resp> = self
            .nats
            .request(
                subjects::AI_SEARCH_QUERIES,
                &serde_json::json!({ "artist": artist, "title": title }),
                Duration::from_secs(40),
                false,
            )
            .await?;
        let queries: Vec<String> = res
            .and_then(|r| r.queries)
            .map(|v| v.into_iter().filter(|q| !q.trim().is_empty()).collect())
            .unwrap_or_default();
        if queries.is_empty() {
            let fallback = format!("{artist} {title}").trim().to_string();
            if fallback.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![fallback])
            }
        } else {
            Ok(queries)
        }
    }

    pub async fn rank_lyrics(
        &self,
        artist: &str,
        title: &str,
        candidates: &[RankCandidate],
    ) -> AppResult<Option<RankResult>> {
        if candidates.is_empty() {
            return Ok(None);
        }
        self.nats
            .request(
                subjects::AI_RANK_LYRICS,
                &serde_json::json!({
                    "artist": artist,
                    "title": title,
                    "candidates": candidates,
                }),
                Duration::from_secs(60),
                false,
            )
            .await
    }

    /// MuLan text vector (512-dim CLAP space). См. [`cached_encode`].
    pub async fn encode_text_mulan(&self, text: &str) -> AppResult<EncodeOutcome> {
        self.cached_encode(&MULAN, text).await
    }

    /// Lyrics query vector (1024-dim bge-m3). Зеркало [`encode_text_mulan`],
    /// другая модель/коллекция.
    pub async fn encode_lyrics_text(&self, text: &str) -> AppResult<EncodeOutcome> {
        self.cached_encode(&LYRICS, text).await
    }

    /// Read-path энкода под хайлоад: **Redis (hot) → Qdrant (durable) → джоб в
    /// work-queue + короткий poll**. Воркер не блокирует запрос: на промахе
    /// публикуем `encode.text.new` (single-flight через in-flight маркер +
    /// `Nats-Msg-Id` дедуп), ждём гибрид-бюджет (~10s), и если результат не
    /// прилетел — отдаём `Preparing` (джоб дольёт кэш в фоне, см.
    /// [`spawn_done_consumer`]). Сам энкод считается ровно один раз и durable.
    async fn cached_encode(&self, m: &EncodeModel, text: &str) -> AppResult<EncodeOutcome> {
        let normalized = text.trim();
        if normalized.is_empty() {
            return Ok(EncodeOutcome::Empty);
        }
        let hash = hex::encode(Sha256::digest(normalized.as_bytes()));
        let cache_key = format!("{}{}", m.prefix, hash);

        // 1. Hot Redis (вектор 30д / негатив-пустышка 2ч).
        if let Some(o) = self.read_cache(&cache_key).await {
            return Ok(o);
        }
        // 2. Durable Qdrant — переживает eviction Redis; на хите греем Redis.
        if let Some(v) = self.qdrant.get_query_vector(m.collection, &hash).await {
            if !v.is_empty() {
                self.store_vector(&cache_key, &v).await;
                return Ok(EncodeOutcome::Ready(v));
            }
        }
        // 3. Промах → диспатчим джоб (single-flight) и коротко поллим кэш.
        let inflight_key = format!("encjob:{}:{}", m.model, hash);
        let won = self
            .cache
            .try_acquire_lock(&inflight_key, ENCODE_INFLIGHT_TTL_SECS)
            .await
            .unwrap_or(true);
        if won {
            let job = serde_json::json!({ "model": m.model, "text": normalized, "hash": hash });
            let msg_id = format!("{}:{}", m.model, hash);
            if let Err(e) = self
                .nats
                .publish_dedup(subjects::ENCODE_TEXT_NEW, &job, &msg_id)
                .await
            {
                // Джоб не уехал → снимаем in-flight лок, иначе следующий запрос
                // того же текста ENCODE_INFLIGHT_TTL_SECS видел бы «занято» и
                // поллил пустой кэш, хотя считать вектор некому.
                warn!(error = %e, "encode job publish failed; releasing in-flight lock");
                let _ = self.cache.release_lock(&inflight_key).await;
            }
        }
        for _ in 0..ENCODE_POLL_TRIES {
            tokio::time::sleep(Duration::from_millis(ENCODE_POLL_INTERVAL_MS)).await;
            if let Some(o) = self.read_cache(&cache_key).await {
                return Ok(o);
            }
        }
        Ok(EncodeOutcome::Preparing)
    }

    /// Консьюмер `done.encode`: воркер посчитал вектор (когда смог) → пишем в
    /// durable Qdrant (только непустой) + hot Redis 30д; пустой → негатив-кэш
    /// 2ч (не durable). Идемпотентно (upsert by-id + set). Брат
    /// `done.embed_lyrics`.
    pub fn spawn_done_consumer(self: &Arc<Self>) {
        let me = self.clone();
        self.nats.consume(
            streams::DONE.name,
            "backend-done-encode",
            Some(subjects::DONE_ENCODE),
            16,
            move |data| {
                let me = me.clone();
                async move {
                    let model = data.get("model").and_then(|v| v.as_str()).unwrap_or("");
                    let hash = data.get("hash").and_then(|v| v.as_str()).unwrap_or("");
                    if hash.is_empty() {
                        return Ok(());
                    }
                    let (prefix, collection) = match model {
                        "mulan" => (MULAN.prefix, MULAN.collection),
                        "lyrics" => (LYRICS.prefix, LYRICS.collection),
                        _ => return Ok(()),
                    };
                    let cache_key = format!("{prefix}{hash}");
                    match parse_f32_vec(data.get("vector")) {
                        // Вектор → Qdrant (durable) ДО Redis: upsert упал → Err →
                        // NAK → передоставка (не теряем дорогой энкод).
                        Some(v) => {
                            me.qdrant
                                .upsert_query_vector(collection, hash, v.clone())
                                .await?;
                            me.store_vector(&cache_key, &v).await;
                        }
                        None => me.store_negative(&cache_key).await,
                    }
                    Ok(())
                }
            },
        );
    }

    /// Hot-кэш чтение: пустой массив = негатив-пустышка → `Empty`.
    async fn read_cache(&self, cache_key: &str) -> Option<EncodeOutcome> {
        if let Ok(Some(raw)) = self.cache.get_raw(cache_key).await {
            if let Ok(v) = serde_json::from_str::<Vec<f32>>(&raw) {
                return Some(if v.is_empty() {
                    EncodeOutcome::Empty
                } else {
                    EncodeOutcome::Ready(v)
                });
            }
        }
        None
    }

    async fn store_vector(&self, cache_key: &str, vec: &[f32]) {
        if let Ok(json) = serde_json::to_string(vec) {
            let _ = self
                .cache
                .set_raw(
                    cache_key,
                    &json,
                    VEC_CACHE_TTL_SECS,
                    None,
                    CacheScope::Shared,
                    None,
                )
                .await;
        }
    }

    async fn store_negative(&self, cache_key: &str) {
        let _ = self
            .cache
            .set_raw(
                cache_key,
                "[]",
                VEC_NEG_TTL_SECS,
                None,
                CacheScope::Shared,
                None,
            )
            .await;
    }

    pub async fn score_quality(&self, features: &[Vec<f32>]) -> AppResult<Option<Vec<f32>>> {
        if features.is_empty() {
            return Ok(Some(Vec::new()));
        }
        #[derive(Deserialize)]
        struct Resp {
            scores: Option<Vec<f32>>,
        }
        let res: Option<Resp> = self
            .nats
            .request(
                subjects::AI_QUALITY_SCORE,
                &serde_json::json!({ "features": features }),
                Duration::from_secs(10),
                false,
            )
            .await?;
        Ok(res.and_then(|r| r.scores))
    }
}
