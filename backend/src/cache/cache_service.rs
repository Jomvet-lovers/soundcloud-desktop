use std::sync::Arc;

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool;
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::error::AppResult;

const DATA_PREFIX: &str = "api:";
const INDEX_PREFIX: &str = "idx:";
const LOCK_PREFIX: &str = "lock:";
const DEL_CHUNK: usize = 500;

#[derive(Clone, Copy, Debug)]
pub enum CacheScope {
    Shared,
    User,
}

impl CacheScope {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::User => "user",
        }
    }
}

pub struct CacheService {
    redis: Pool,
}

impl CacheService {
    pub fn new(redis: Pool) -> Arc<Self> {
        Arc::new(Self { redis })
    }

    /// Lightweight Redis liveness: a GET that confirms the pool can round-trip.
    pub async fn ping(&self) -> bool {
        self.get_raw("__healthcheck__").await.is_ok()
    }

    /// deadpool counters: (size = in-use + idle, available, max_size).
    pub fn pool_status(&self) -> (usize, usize, usize) {
        let s = self.redis.status();
        (s.size, s.available, s.max_size)
    }

    pub fn build_key(
        &self,
        method: &str,
        url: &str,
        scope: CacheScope,
        session_id: Option<&str>,
    ) -> String {
        let (path, query) = match url.split_once('?') {
            Some((p, q)) => (p, q),
            None => (url, ""),
        };
        let mut qparts: Vec<&str> = query.split('&').filter(|s| !s.is_empty()).collect();
        qparts.sort_unstable();
        let sorted = qparts.join("&");

        let raw = match scope {
            CacheScope::User => {
                format!("user:{method}:{path}:{sorted}:{}", session_id.unwrap_or(""))
            }
            CacheScope::Shared => format!("shared:{method}:{path}:{sorted}"),
        };
        let digest = Sha256::digest(raw.as_bytes());
        hex::encode(digest)
    }

    pub async fn get_raw(&self, key: &str) -> AppResult<Option<String>> {
        let mut conn = self.redis.get().await?;
        let full = format!("{DATA_PREFIX}{key}");
        let v: Option<String> = conn.get(&full).await?;
        Ok(v)
    }

    pub async fn set_raw(
        &self,
        key: &str,
        payload: &str,
        ttl_sec: u64,
        cache_key: Option<&str>,
        scope: CacheScope,
        session_id: Option<&str>,
    ) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let full = format!("{DATA_PREFIX}{key}");

        let mut pipe = deadpool_redis::redis::pipe();
        pipe.atomic()
            .set_ex::<_, _>(&full, payload, ttl_sec)
            .ignore();

        if let Some(ck) = cache_key {
            let index_key = build_index_key(ck, scope, session_id);
            let now_ms = chrono::Utc::now().timestamp_millis();
            let expire_at = now_ms + (ttl_sec as i64) * 1000;
            pipe.zadd(&index_key, key, expire_at).ignore();
            pipe.zrembyscore::<_, _, _>(&index_key, 0, now_ms).ignore();
            pipe.pexpire_at(&index_key, expire_at).ignore();
        }

        pipe.query_async::<()>(&mut conn).await?;
        Ok(())
    }

    pub async fn clear_by_cache_keys(
        &self,
        cache_keys: &[String],
        session_id: Option<&str>,
    ) -> AppResult<()> {
        let mut seen = std::collections::BTreeSet::new();
        for k in cache_keys {
            let trimmed = k.trim();
            if !trimmed.is_empty() {
                seen.insert(trimmed.to_string());
            }
        }
        if seen.is_empty() {
            return Ok(());
        }

        let mut index_keys: Vec<String> = Vec::with_capacity(seen.len() * 2);
        for ck in &seen {
            index_keys.push(build_index_key(ck, CacheScope::Shared, None));
            if session_id.is_some() {
                index_keys.push(build_index_key(ck, CacheScope::User, session_id));
            }
        }

        for idx in index_keys {
            if let Err(e) = self.clear_index(&idx).await {
                warn!(index = %idx, error = %e, "cache clear_index failed");
            }
        }
        Ok(())
    }

    /// SETNX-лок. Возвращает true если лок захвачен этим воркером.
    /// Используется для дедупа фоновых refresh-task'ов: ключ вида
    /// `refresh:user_likes_tracks:{user_id}` живёт TTL, лишние spawn'ы
    /// видят занято и тихо отваливаются. Освобождать необязательно —
    /// TTL сам делает это.
    pub async fn try_acquire_lock(&self, key: &str, ttl_sec: u64) -> AppResult<bool> {
        let mut conn = self.redis.get().await?;
        let full = format!("{LOCK_PREFIX}{key}");
        let acquired: Option<String> = deadpool_redis::redis::cmd("SET")
            .arg(&full)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(ttl_sec)
            .query_async(&mut conn)
            .await?;
        Ok(acquired.is_some())
    }

    /// Снимает SETNX-лок досрочно. Нужно когда фоновая работа под локом
    /// сорвалась (например джоб не опубликовался) и переспрос должен
    /// пере-диспатчиться сразу, не дожидаясь TTL.
    pub async fn release_lock(&self, key: &str) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let full = format!("{LOCK_PREFIX}{key}");
        let _: () = conn.del(full).await?;
        Ok(())
    }

    async fn clear_index(&self, index_key: &str) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let members: Vec<String> = conn.zrange(index_key, 0, -1).await?;
        if members.is_empty() {
            let _: () = conn.del(index_key).await?;
            return Ok(());
        }

        for chunk in members.chunks(DEL_CHUNK) {
            let keys: Vec<String> = chunk.iter().map(|m| format!("{DATA_PREFIX}{m}")).collect();
            let _: () = conn.del(keys).await?;
        }
        let _: () = conn.del(index_key).await?;
        Ok(())
    }
}

pub fn build_index_key(cache_key: &str, scope: CacheScope, session_id: Option<&str>) -> String {
    match scope {
        CacheScope::User => format!(
            "{INDEX_PREFIX}{}:{}:{}",
            scope.as_str(),
            session_id.unwrap_or(""),
            cache_key
        ),
        CacheScope::Shared => format!("{INDEX_PREFIX}{}:{}", scope.as_str(), cache_key),
    }
}
