//! AI fuzzy-matcher (backend ↔ worker через NATS request-reply).
//!
//! Используется matcher-пайплайном в borderline-зоне (когда алгоритмический
//! score 0.45..0.7 — недостаточно уверенно, чтобы линковать, но и не явный
//! mismatch). LLM в worker'е получает компактный промпт «target vs N
//! кандидатов» и возвращает индекс совпавшего (или null). Ответ кэшируется
//! в Redis на 30 дней — повторные wanted-tick'и не плодят запросов.
//!
//! Бюджет берётся из общего daily-бюджета AI (тот же, что у `AiResolverClient`).

use std::sync::Arc;
use std::time::Duration;

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::bus::nats::NatsService;
use crate::bus::subjects;
use crate::error::AppResult;

const CACHE_TTL_SEC: u64 = 30 * 24 * 60 * 60;
const CACHE_PREFIX: &str = "ai:match:";
const BUDGET_PREFIX: &str = "ai:resolve:budget:"; // общий счётчик с AiResolverClient

#[derive(Debug, Clone, Serialize)]
pub struct MatchTarget<'a> {
    pub artist: &'a str,
    pub title: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchCandidate<'a> {
    /// Внутренний идентификатор кандидата в текущем запросе (0..n-1). Worker
    /// возвращает его обратно.
    pub id: u32,
    pub artist: &'a str,
    pub title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploader: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<i32>,
}

#[derive(Debug, Serialize)]
struct MatchRequest<'a> {
    target: MatchTarget<'a>,
    candidates: &'a [MatchCandidate<'a>],
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MatchReply {
    /// id из запроса; null если ни один не совпал.
    #[serde(default)]
    match_id: Option<u32>,
    #[serde(default)]
    confidence: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct AiMatch {
    pub candidate_id: u32,
    pub confidence: f32,
}

pub struct AiMatcherClient {
    nats: Arc<NatsService>,
    redis: RedisPool,
    timeout: Duration,
    daily_budget: u64,
}

impl AiMatcherClient {
    pub fn new(
        nats: Arc<NatsService>,
        redis: RedisPool,
        timeout_ms: u64,
        daily_budget: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            nats,
            redis,
            timeout: Duration::from_millis(timeout_ms.max(1000)),
            daily_budget,
        })
    }

    /// Спрашивает worker «какой из candidates это target»; None если не
    /// определилось / бюджет исчерпан / NATS таймаут.
    pub async fn pick(
        &self,
        target: MatchTarget<'_>,
        candidates: &[MatchCandidate<'_>],
    ) -> AppResult<Option<AiMatch>> {
        if candidates.is_empty() || target.title.trim().is_empty() {
            return Ok(None);
        }
        let cache_key = format!("{CACHE_PREFIX}{}", request_hash(&target, candidates));
        if let Some(reply) = self.cache_get(&cache_key).await {
            return Ok(reply_to_match(reply));
        }
        if !self.budget_take().await {
            debug!("AI match daily budget exceeded, skipping");
            return Ok(None);
        }
        let req = MatchRequest { target, candidates };
        let reply: Option<MatchReply> = self
            .nats
            .request(subjects::AI_MATCH_TRACK, &req, self.timeout, false)
            .await?;
        let Some(r) = reply else {
            return Ok(None);
        };
        let _ = self.cache_set(&cache_key, &r).await;
        Ok(reply_to_match(r))
    }

    async fn cache_get(&self, key: &str) -> Option<MatchReply> {
        let mut conn = self.redis.get().await.ok()?;
        let raw: Option<String> = conn.get(key).await.ok()?;
        serde_json::from_str(&raw?).ok()
    }

    async fn cache_set(&self, key: &str, reply: &MatchReply) -> AppResult<()> {
        let payload = serde_json::to_string(reply)
            .map_err(|e| crate::error::AppError::internal(format!("ai cache encode: {e}")))?;
        let mut conn = self.redis.get().await?;
        let _: () = conn.set_ex(key, payload, CACHE_TTL_SEC).await?;
        Ok(())
    }

    async fn budget_take(&self) -> bool {
        if self.daily_budget == 0 {
            return true;
        }
        let date = chrono::Utc::now().format("%Y%m%d").to_string();
        let key = format!("{BUDGET_PREFIX}{date}");
        let mut conn = match self.redis.get().await {
            Ok(c) => c,
            Err(_) => return true,
        };
        let count: i64 = match conn.incr(&key, 1).await {
            Ok(n) => n,
            Err(_) => return true,
        };
        if count == 1 {
            let _: Result<(), _> = conn.expire(&key, 60 * 60 * 30).await;
        }
        (count as u64) <= self.daily_budget
    }
}

fn reply_to_match(r: MatchReply) -> Option<AiMatch> {
    let id = r.match_id?;
    let conf = r.confidence.unwrap_or(0.7).clamp(0.3, 0.95);
    Some(AiMatch {
        candidate_id: id,
        confidence: conf,
    })
}

fn request_hash(target: &MatchTarget, candidates: &[MatchCandidate]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(target.artist.as_bytes());
    hasher.update(b"\x00");
    hasher.update(target.title.as_bytes());
    hasher.update(b"\x00");
    for c in candidates {
        hasher.update(c.id.to_le_bytes());
        hasher.update(c.artist.as_bytes());
        hasher.update(b"\x00");
        hasher.update(c.title.as_bytes());
        hasher.update(b"\x00");
        hasher.update(c.uploader.unwrap_or("").as_bytes());
        hasher.update(b"\x00");
        if let Some(d) = c.duration_sec {
            hasher.update(d.to_le_bytes());
        }
        hasher.update(b"\x01");
    }
    hex::encode(hasher.finalize())
}
