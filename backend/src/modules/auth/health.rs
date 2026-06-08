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
const PENALTY_BASE_SEC: u64 = 60;
const PENALTY_MAX_SEC: u64 = 30 * 60;
const PENALTY_STRIKE_TTL_SEC: i64 = 60 * 60;
/// Окно per-app счётчика выпущенных токенов (SC-лимит 50/12ч/app).
const ISSUE_WINDOW_SEC: i64 = 12 * 60 * 60;

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
        let _: () = conn
            .del(&[
                format!("auth:app:{app_id}:penalty"),
                format!("auth:app:{app_id}:strikes"),
            ])
            .await?;
        Ok(())
    }

    pub async fn penalize_app(&self, app_id: &str) -> AppResult<u64> {
        let mut conn = self.redis.get().await?;
        let strikes_key = format!("auth:app:{app_id}:strikes");
        let strikes: i64 = conn.incr(&strikes_key, 1).await?;
        let _: () = conn.expire(&strikes_key, PENALTY_STRIKE_TTL_SEC).await?;
        let exp = (strikes.max(1) - 1).clamp(0, 20) as u32;
        let cooldown = PENALTY_BASE_SEC
            .saturating_mul(2u64.saturating_pow(exp))
            .min(PENALTY_MAX_SEC);
        let _: () = conn
            .set_ex(format!("auth:app:{app_id}:penalty"), strikes, cooldown)
            .await?;
        Ok(cooldown)
    }

    pub async fn app_penalties(&self, app_ids: &[String]) -> AppResult<HashMap<String, i64>> {
        if app_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conn = self.redis.get().await?;
        let mut pipe = deadpool_redis::redis::pipe();
        for id in app_ids {
            pipe.pttl(format!("auth:app:{id}:penalty"));
        }
        let ttls: Vec<i64> = pipe.query_async(&mut conn).await?;
        let mut out = HashMap::new();
        for (i, id) in app_ids.iter().enumerate() {
            if let Some(&ms) = ttls.get(i) {
                if ms > 0 {
                    out.insert(id.clone(), (ms / 1000).max(1));
                }
            }
        }
        Ok(out)
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
        let trimmed: String = error.chars().take(500).collect();
        // Кодируем вид в значение (`<tag>:<msg>`), чтобы cached-hit вернул
        // правильный HTTP-статус (502/429/401), а не всегда 401.
        let value = format!("{}:{}", kind.tag(), trimmed);
        let _: () = conn
            .set_ex(
                format!("auth:refresh-fail:{session_id}"),
                value,
                kind.ttl_sec(),
            )
            .await?;
        Ok(())
    }

    pub async fn get_cached_refresh_failure(
        &self,
        session_id: &str,
    ) -> AppResult<Option<(RefreshFailKind, String)>> {
        let mut conn = self.redis.get().await?;
        let v: Option<String> = conn.get(format!("auth:refresh-fail:{session_id}")).await?;
        Ok(v.map(|s| {
            let mut chars = s.chars();
            let tag = chars.next().unwrap_or('T');
            let msg = chars.as_str().strip_prefix(':').unwrap_or("").to_string();
            (RefreshFailKind::from_tag(tag), msg)
        }))
    }

    pub async fn clear_refresh_failure(&self, session_id: &str) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let _: () = conn.del(format!("auth:refresh-fail:{session_id}")).await?;
        Ok(())
    }

    /// Инкремент 12ч-счётчика выпущенных токенов на app (SC per-app 50/12ч).
    pub async fn record_token_issue(&self, app_id: &str) -> AppResult<()> {
        let mut conn = self.redis.get().await?;
        let key = format!("auth:app:{app_id}:issued12h");
        let n: i64 = conn.incr(&key, 1).await?;
        if n == 1 {
            let _: () = conn.expire(&key, ISSUE_WINDOW_SEC).await?;
        }
        Ok(())
    }

    /// Batch-чтение 12ч-issuance по apps — для распределения новых логинов
    /// под per-app бюджет (рефреш привязан к issuing-app и не редистрибутируется).
    pub async fn tokens_issued_12h(&self, app_ids: &[String]) -> AppResult<HashMap<String, i64>> {
        if app_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut conn = self.redis.get().await?;
        let mut pipe = deadpool_redis::redis::pipe();
        for id in app_ids {
            pipe.get(format!("auth:app:{id}:issued12h"));
        }
        let vals: Vec<Option<String>> = pipe.query_async(&mut conn).await?;
        let mut out = HashMap::with_capacity(app_ids.len());
        for (i, id) in app_ids.iter().enumerate() {
            let c = vals
                .get(i)
                .and_then(|v| v.as_ref())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            out.insert(id.clone(), c);
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RefreshFailKind {
    /// Транзиентный сбой роута/сети — тихо ретраить, НЕ ре-логин (фронту 502).
    Transient,
    /// SC rate-limit (429) — подождать (фронту 429, без модалки).
    RateLimit,
    /// SC отверг refresh_token (400/401) — нужен ре-логин (фронту 401 → модалка).
    ReAuth,
}

impl RefreshFailKind {
    fn tag(self) -> char {
        match self {
            Self::Transient => 'T',
            Self::RateLimit => 'R',
            Self::ReAuth => 'A',
        }
    }

    fn from_tag(c: char) -> Self {
        match c {
            'R' => Self::RateLimit,
            'A' => Self::ReAuth,
            _ => Self::Transient,
        }
    }

    fn ttl_sec(self) -> u64 {
        match self {
            Self::Transient => REFRESH_FAIL_TTL_SEC,
            Self::RateLimit | Self::ReAuth => RATE_LIMIT_FAIL_TTL_SEC,
        }
    }
}
