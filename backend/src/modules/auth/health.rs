//! Sliding-window health-метрики per OAuth-app + memoize последних refresh-fail
//! per session. Цель — погасить retry-storm на /refresh: если SC-прокси сдох,
//! мы не должны лупить запросы туда же на каждый HTTP-вход; одной ошибки
//! достаточно, чтобы заглушить 60 секунд следующих попыток.
//!
//! Метрики per-app собираются в скользящем окне (`WINDOW_SEC`): счётчики
//! success/failure хранятся в Redis с TTL=окно. `is_unhealthy` смотрит на
//! ошибочность за последний период и при наличии стат-значимости (минимум
//! `MIN_SAMPLES` попыток) считает app плохим.

use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::AppResult;

const WINDOW_SEC: u64 = 300;
const REFRESH_FAIL_TTL_SEC: u64 = 60;
const RATE_LIMIT_FAIL_TTL_SEC: u64 = 5 * 60;
const MIN_SAMPLES: i64 = 10;
const UNHEALTHY_RATIO: f64 = 0.5;

#[derive(Debug, Clone, Default)]
pub struct AppHealth {
    pub successes: i64,
    pub failures: i64,
}

impl AppHealth {
    pub fn total(&self) -> i64 {
        self.successes + self.failures
    }
    pub fn unhealthy(&self) -> bool {
        let total = self.total();
        if total < MIN_SAMPLES {
            return false;
        }
        (self.failures as f64) / (total as f64) > UNHEALTHY_RATIO
    }
}

pub struct AuthHealthService {
    redis: RedisPool,
}

impl AuthHealthService {
    pub fn new(redis: RedisPool) -> Arc<Self> {
        Arc::new(Self { redis })
    }

    pub async fn record_app_success(&self, app_id: &str) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let key = format!("auth:app:{app_id}:success");
        let _: i64 = conn.incr(&key, 1).await?;
        let _: () = conn.expire(&key, WINDOW_SEC as i64).await?;
        Ok(())
    }

    pub async fn record_app_failure(&self, app_id: &str) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let key = format!("auth:app:{app_id}:failure");
        let _: i64 = conn.incr(&key, 1).await?;
        let _: () = conn.expire(&key, WINDOW_SEC as i64).await?;
        Ok(())
    }

    /// Batch-чтение health-счётчиков для нескольких apps одним pipeline.
    /// На login-флоу заменяет 2N последовательных GET (N = число активных
    /// OAuth-apps).
    pub async fn app_healths(&self, app_ids: &[String]) -> AppResult<HashMap<String, AppHealth>> {
        if app_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conn = self.redis.get().await?;
        let mut pipe = deadpool_redis::redis::pipe();
        for id in app_ids {
            pipe.get(format!("auth:app:{id}:success"))
                .get(format!("auth:app:{id}:failure"));
        }
        let flat: Vec<Option<String>> = pipe.query_async(&mut conn).await?;
        let mut out = HashMap::with_capacity(app_ids.len());
        for (i, id) in app_ids.iter().enumerate() {
            let s = flat.get(i * 2).and_then(|v| v.as_ref());
            let f = flat.get(i * 2 + 1).and_then(|v| v.as_ref());
            out.insert(
                id.clone(),
                AppHealth {
                    successes: s.and_then(|s| s.parse().ok()).unwrap_or(0),
                    failures: f.and_then(|s| s.parse().ok()).unwrap_or(0),
                },
            );
        }
        Ok(out)
    }

    /// Запомнить последний refresh-fail для сессии. Следующий запрос на refresh
    /// в течение TTL отдаст эту ошибку без обращения к SC.
    pub async fn cache_refresh_failure(
        &self,
        session_id: &str,
        error: &str,
        kind: RefreshFailKind,
    ) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let ttl = match kind {
            RefreshFailKind::Generic => REFRESH_FAIL_TTL_SEC,
            RefreshFailKind::RateLimit => RATE_LIMIT_FAIL_TTL_SEC,
        };
        let trimmed: String = error.chars().take(500).collect();
        let _: () = conn
            .set_ex(format!("auth:refresh-fail:{session_id}"), trimmed, ttl)
            .await?;
        Ok(())
    }

    pub async fn get_cached_refresh_failure(&self, session_id: &str) -> AppResult<Option<String>> {
        let mut conn = self.redis.get().await?;
        let v: Option<String> = conn.get(format!("auth:refresh-fail:{session_id}")).await?;
        Ok(v)
    }

    pub async fn clear_refresh_failure(&self, session_id: &str) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let _: () = conn.del(format!("auth:refresh-fail:{session_id}")).await?;
        Ok(())
    }
}

pub enum RefreshFailKind {
    Generic,
    RateLimit,
}
