use std::sync::Arc;
use std::time::Duration;

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::bus::nats::NatsService;
use crate::bus::subjects::subjects;
use crate::error::AppResult;
use crate::modules::enrich::resolver::{
    AlbumCandidate, ArtistCandidate, ResolveResult, ResolveSource, TrackContext,
};

const CACHE_TTL_SEC: u64 = 30 * 24 * 60 * 60;
const VERIFY_CACHE_TTL_SEC: u64 = 7 * 24 * 60 * 60;
const CACHE_PREFIX: &str = "ai:resolve:";
const VERIFY_CACHE_PREFIX: &str = "ai:verify:";
const BUDGET_PREFIX: &str = "ai:resolve:budget:";

#[derive(Debug, Serialize)]
pub struct AiResolveRequest<'a> {
    pub title: &'a str,
    pub uploader: Option<&'a str>,
    pub duration_ms: Option<i32>,
    pub isrc: Option<&'a str>,
    pub metadata_artist: Option<&'a str>,
    pub description: Option<&'a str>,
    pub raw: &'a Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiResolveReply {
    #[serde(default)]
    primary_artist: Option<String>,
    #[serde(default)]
    featured: Vec<String>,
    #[serde(default)]
    producers: Vec<String>,
    #[serde(default)]
    remixers: Vec<String>,
    #[serde(default)]
    album: Option<AiAlbum>,
    #[serde(default)]
    confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiAlbum {
    title: String,
    #[serde(default)]
    year: Option<i16>,
    #[serde(default)]
    primary_artist: Option<String>,
}

#[derive(Debug, Serialize)]
struct AiVerifyRequest<'a> {
    artist: &'a str,
    title: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiVerifyReply {
    #[serde(default)]
    exists: Option<bool>,
    #[serde(default)]
    confidence: Option<f32>,
}

pub struct AiResolverClient {
    nats: Arc<NatsService>,
    redis: RedisPool,
    timeout: Duration,
    daily_budget: u64,
}

impl AiResolverClient {
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

    pub async fn resolve(
        &self,
        ctx: &TrackContext,
        raw: &Value,
    ) -> AppResult<Option<ResolveResult>> {
        let cache_key = format!("{CACHE_PREFIX}{}", context_hash(ctx));
        if let Some(reply) = self.cache_get(&cache_key).await {
            return Ok(self.build_result(reply, ctx));
        }
        if !self.budget_take().await {
            debug!("AI resolve daily budget exceeded, skipping");
            return Ok(None);
        }
        let req = AiResolveRequest {
            title: ctx.title.as_str(),
            uploader: ctx.uploader_username.as_deref(),
            duration_ms: ctx.duration_ms,
            isrc: ctx.isrc.as_deref(),
            metadata_artist: ctx.metadata_artist.as_deref(),
            description: ctx.description.as_deref(),
            raw,
        };
        let reply: Option<AiResolveReply> = self
            .nats
            .request(subjects::AI_RESOLVE_ARTIST, &req, self.timeout, false)
            .await?;
        let Some(r) = reply else {
            return Ok(None);
        };
        let _ = self.cache_set(&cache_key, &r).await;
        Ok(self.build_result(r, ctx))
    }

    fn build_result(&self, r: AiResolveReply, ctx: &TrackContext) -> Option<ResolveResult> {
        let primary_name = r
            .primary_artist
            .as_ref()
            .filter(|s| !s.trim().is_empty())?
            .clone();

        let to_cand = |n: String| ArtistCandidate {
            name: n,
            ..ArtistCandidate::default()
        };

        let primary = vec![to_cand(primary_name)];
        let featured = r
            .featured
            .into_iter()
            .filter(|n| !n.trim().is_empty())
            .map(to_cand)
            .collect();
        let producers = r
            .producers
            .into_iter()
            .filter(|n| !n.trim().is_empty())
            .map(to_cand)
            .collect();
        let remixers = r
            .remixers
            .into_iter()
            .filter(|n| !n.trim().is_empty())
            .map(to_cand)
            .collect();
        let album = r.album.map(|a| AlbumCandidate {
            title: a.title,
            year: a.year,
            mb_id: None,
            genius_id: None,
            cover_url: None,
            release_type: None,
            primary_artist: a.primary_artist.map(to_cand),
        });
        let confidence = r.confidence.unwrap_or(0.55).clamp(0.3, 0.75);
        Some(ResolveResult {
            source: ResolveSource::Ai,
            confidence,
            primary,
            featured,
            producers,
            remixers,
            album,
            isrc: ctx.isrc.clone(),
        })
    }

    async fn cache_get(&self, key: &str) -> Option<AiResolveReply> {
        let mut conn = self.redis.get().await.ok()?;
        let raw: Option<String> = conn.get(key).await.ok()?;
        let raw = raw?;
        serde_json::from_str(&raw).ok()
    }

    async fn cache_set(&self, key: &str, reply: &AiResolveReply) -> AppResult<()> {
        let payload = serde_json::to_string(reply)
            .map_err(|e| crate::error::AppError::internal(format!("ai cache encode: {e}")))?;
        let mut conn = self.redis.get().await?;
        let _: () = conn.set_ex(key, payload, CACHE_TTL_SEC).await?;
        Ok(())
    }

    pub async fn verify_existence(
        &self,
        artist: &str,
        title: &str,
    ) -> AppResult<Option<bool>> {
        if artist.trim().is_empty() || title.trim().is_empty() {
            return Ok(None);
        }
        let cache_key = format!("{VERIFY_CACHE_PREFIX}{}", verify_hash(artist, title));
        if let Some(cached) = self.verify_cache_get(&cache_key).await {
            return Ok(cached);
        }
        if !self.budget_take().await {
            debug!("AI verify daily budget exceeded, skipping");
            return Ok(None);
        }
        let req = AiVerifyRequest { artist, title };
        let reply: Option<AiVerifyReply> = self
            .nats
            .request(subjects::AI_VERIFY_EXISTENCE, &req, self.timeout, false)
            .await?;
        let exists = reply.and_then(|r| r.exists);
        if let Some(v) = exists {
            let _ = self.verify_cache_set(&cache_key, v).await;
        }
        Ok(exists)
    }

    async fn verify_cache_get(&self, key: &str) -> Option<Option<bool>> {
        let mut conn = self.redis.get().await.ok()?;
        let raw: Option<String> = conn.get(key).await.ok()?;
        let raw = raw?;
        match raw.as_str() {
            "1" => Some(Some(true)),
            "0" => Some(Some(false)),
            _ => None,
        }
    }

    async fn verify_cache_set(&self, key: &str, value: bool) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let payload = if value { "1" } else { "0" };
        let _: () = conn.set_ex(key, payload, VERIFY_CACHE_TTL_SEC).await?;
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
            // Чуть больше суток (25h) — буфер на дрейф часов и редкие миграции
            // ключа между нодами Redis. Бюджет всё равно сбрасывается по %Y%m%d.
            let _: Result<(), _> = conn.expire(&key, 60 * 60 * 25).await;
        }
        (count as u64) <= self.daily_budget
    }
}

fn verify_hash(artist: &str, title: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(artist.trim().to_lowercase().as_bytes());
    hasher.update(b"\x00");
    hasher.update(title.trim().to_lowercase().as_bytes());
    hex::encode(hasher.finalize())
}

fn context_hash(ctx: &TrackContext) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ctx.title.as_bytes());
    hasher.update(b"\x00");
    hasher.update(ctx.uploader_username.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"\x00");
    if let Some(d) = ctx.duration_ms {
        hasher.update(d.to_le_bytes());
    }
    hasher.update(b"\x00");
    hasher.update(ctx.isrc.as_deref().unwrap_or("").as_bytes());
    hex::encode(hasher.finalize())
}
