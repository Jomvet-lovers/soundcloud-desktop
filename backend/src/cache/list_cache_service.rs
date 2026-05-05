use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool;
use mini_moka::sync::Cache;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;
use tracing::warn;

use crate::cache::cache_service::CacheScope;
use crate::error::AppResult;

const LIST_PREFIX: &str = "list:";
const DEFAULT_CHUNK_SIZE: usize = 200;
const MAX_CHUNKS_PER_REQUEST: usize = 8;
const INFLIGHT_CAPACITY: u64 = 65_536;
const INFLIGHT_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPageResult<T> {
    pub collection: Vec<T>,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct FetchChunkResult<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ListState<T> {
    items: Vec<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
    exhausted: bool,
}

impl<T> Default for ListState<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
            exhausted: false,
        }
    }
}

pub struct GetPageOptions<'a> {
    pub key: &'a str,
    pub scope: CacheScope,
    pub session_id: Option<&'a str>,
    pub ttl_sec: u64,
    pub page: i64,
    pub limit: i64,
    pub chunk_size: Option<usize>,
}

pub struct ListCacheService {
    redis: Pool,
    inflight: Cache<String, Arc<AsyncMutex<()>>>,
}

impl ListCacheService {
    pub fn new(redis: Pool) -> Arc<Self> {
        Arc::new(Self {
            redis,
            inflight: Cache::builder()
                .max_capacity(INFLIGHT_CAPACITY)
                .time_to_idle(INFLIGHT_TTL)
                .build(),
        })
    }

    fn build_redis_key(key: &str, scope: CacheScope, session_id: Option<&str>) -> String {
        match scope {
            CacheScope::User => format!("user:{}:{key}", session_id.unwrap_or("")),
            CacheScope::Shared => format!("shared:{key}"),
        }
    }

    fn lock(&self, key: &str) -> Arc<AsyncMutex<()>> {
        if let Some(lock) = self.inflight.get(&key.to_string()) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        self.inflight.insert(key.to_string(), lock.clone());
        lock
    }

    async fn load<T>(&self, redis_key: &str) -> ListState<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut conn = match self.redis.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(key = %redis_key, error = %e, "list-cache get connection failed");
                return ListState::default();
            }
        };
        let full = format!("{LIST_PREFIX}{redis_key}");
        let raw: Option<String> = match conn.get(&full).await {
            Ok(v) => v,
            Err(e) => {
                warn!(key = %redis_key, error = %e, "list-cache get failed");
                return ListState::default();
            }
        };
        match raw {
            None => ListState::default(),
            Some(s) => match serde_json::from_str::<ListState<T>>(&s) {
                Ok(st) => st,
                Err(_) => {
                    let _: Result<(), _> = conn.del(&full).await;
                    ListState::default()
                }
            },
        }
    }

    async fn save<T: Serialize>(&self, redis_key: &str, state: &ListState<T>, ttl_sec: u64) {
        let body = match serde_json::to_string(state) {
            Ok(s) => s,
            Err(e) => {
                warn!(key = %redis_key, error = %e, "list-cache serialize failed");
                return;
            }
        };
        let mut conn = match self.redis.get().await {
            Ok(c) => c,
            Err(e) => {
                warn!(key = %redis_key, error = %e, "list-cache save connection failed");
                return;
            }
        };
        let full = format!("{LIST_PREFIX}{redis_key}");
        if let Err(e) = conn.set_ex::<_, _, ()>(&full, body, ttl_sec).await {
            warn!(key = %redis_key, error = %e, "list-cache set failed");
        }
    }

    pub async fn invalidate_by_prefixes(
        &self,
        prefixes: &[&str],
        session_id: Option<&str>,
    ) -> AppResult<()> {
        if prefixes.is_empty() {
            return Ok(());
        }
        let mut conn = self.redis.get().await?;
        let mut patterns: Vec<String> = Vec::new();
        for p in prefixes {
            patterns.push(format!(
                "{LIST_PREFIX}{}*",
                Self::build_redis_key(p, CacheScope::Shared, None)
            ));
            if session_id.is_some() {
                patterns.push(format!(
                    "{LIST_PREFIX}{}*",
                    Self::build_redis_key(p, CacheScope::User, session_id)
                ));
            }
        }
        for pattern in patterns {
            let mut cursor: u64 = 0;
            loop {
                let (next, keys): (u64, Vec<String>) = deadpool_redis::redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&pattern)
                    .arg("COUNT")
                    .arg(200)
                    .query_async(&mut conn)
                    .await?;
                if !keys.is_empty() {
                    let _: () = conn.del(keys).await?;
                }
                if next == 0 {
                    break;
                }
                cursor = next;
            }
        }
        Ok(())
    }

    pub async fn invalidate_by_cache_keys(
        &self,
        cache_keys: &[String],
        session_id: Option<&str>,
    ) -> AppResult<()> {
        let mut seen = std::collections::BTreeSet::new();
        for k in cache_keys {
            let t = k.trim();
            if !t.is_empty() {
                seen.insert(t.to_string());
            }
        }
        if seen.is_empty() {
            return Ok(());
        }

        let mut keys: Vec<String> = Vec::with_capacity(seen.len() * 2);
        for k in &seen {
            keys.push(format!(
                "{LIST_PREFIX}{}",
                Self::build_redis_key(k, CacheScope::Shared, None)
            ));
            if session_id.is_some() {
                keys.push(format!(
                    "{LIST_PREFIX}{}",
                    Self::build_redis_key(k, CacheScope::User, session_id)
                ));
            }
        }

        let mut conn = self.redis.get().await?;
        let _: () = conn.del(keys).await?;
        Ok(())
    }

    pub async fn get_page<T, F, Fut>(
        &self,
        opts: GetPageOptions<'_>,
        fetcher: F,
    ) -> AppResult<ListPageResult<T>>
    where
        T: Serialize + for<'de> Deserialize<'de> + Clone,
        F: Fn(Option<String>, usize) -> Fut,
        Fut: Future<Output = AppResult<FetchChunkResult<T>>>,
    {
        let chunk_size = opts.chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);
        let redis_key = Self::build_redis_key(opts.key, opts.scope, opts.session_id);
        let need = ((opts.page + 1) * opts.limit) as usize;

        let mut state: ListState<T> = self.load(&redis_key).await;

        if state.items.len() >= need || state.exhausted {
            return Ok(slice_page(state, opts.page, opts.limit));
        }

        let lock = self.lock(&redis_key);
        let _g = lock.lock().await;

        // re-read под локом — другой воркер мог уже добрать
        state = self.load(&redis_key).await;

        let mut chunks = 0usize;
        while state.items.len() < need && !state.exhausted && chunks < MAX_CHUNKS_PER_REQUEST {
            let fetched = fetcher(state.next_cursor.clone(), chunk_size).await?;
            let items_len = fetched.items.len();
            state.items.extend(fetched.items);
            state.next_cursor = fetched.next_cursor;
            state.exhausted = state.next_cursor.is_none() || items_len == 0;
            chunks += 1;
        }

        if chunks > 0 {
            self.save(&redis_key, &state, opts.ttl_sec).await;
        }

        Ok(slice_page(state, opts.page, opts.limit))
    }
}

fn slice_page<T: Clone>(state: ListState<T>, page: i64, limit: i64) -> ListPageResult<T> {
    let start = (page * limit) as usize;
    let end = start + limit as usize;
    let total = state.items.len();
    let collection: Vec<T> = if start >= total {
        Vec::new()
    } else {
        state.items[start..end.min(total)].to_vec()
    };
    let has_more = !state.exhausted || end < total;
    ListPageResult {
        collection,
        page,
        page_size: limit,
        has_more,
    }
}

pub fn extract_sc_cursor(next_href: Option<&str>) -> Option<String> {
    let href = next_href?;
    let url = url::Url::parse(href).ok()?;
    for (k, v) in url.query_pairs() {
        if k == "cursor" || k == "offset" {
            return Some(v.into_owned());
        }
    }
    None
}

pub fn build_list_cache_key(prefix: &str, params: &[(&str, String)]) -> String {
    let mut parts: Vec<String> = params
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    parts.sort();
    if parts.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}:{}", parts.join("&"))
    }
}
