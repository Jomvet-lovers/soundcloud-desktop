//! Channel health + combinators for high-load SC fetching across two channels.
//!
//! A request can be served by the relay (primary) or the proxy/token chain (backup).
//! Firing both on every request doubles upstream load for no extra throughput, so the
//! default is a HEDGE: primary alone, backup only if it is slow or fails — ~1x
//! upstream load when the primary is healthy, race-like reliability when it isn't.

use std::future::Future;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};

/// How a primary and a backup channel are combined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchStrategy {
    /// Primary only; backup on failure. Fewest SC hits, highest tail latency.
    Fallback,
    /// Both fired together, first success wins. Lowest latency, ~2x SC hits.
    Race,
    /// Primary alone unless slow/failed past the hedge delay, then add the backup.
    Hedge,
}

impl FetchStrategy {
    pub fn from_env() -> Self {
        match std::env::var("CALL_FETCH_STRATEGY").as_deref() {
            Ok("fallback") => Self::Fallback,
            Ok("race") => Self::Race,
            _ => Self::Hedge,
        }
    }
}

/// Consecutive failures before a channel's breaker opens.
const BAN_THRESHOLD: u32 = 4;
/// How long a tripped channel is skipped before being probed again.
const COOLDOWN_MS: i64 = 60_000;

/// Per-channel circuit breaker. A channel that keeps failing is skipped for a
/// cooldown so we stop feeding it, then retried. Transient failures do not trip it.
#[derive(Default)]
pub struct ChannelHealth {
    consecutive_bans: AtomicU32,
    open_until_ms: AtomicI64,
}

impl ChannelHealth {
    pub fn is_open(&self) -> bool {
        now_ms() < self.open_until_ms.load(Ordering::Acquire)
    }

    pub fn record_ok(&self) {
        self.consecutive_bans.store(0, Ordering::Release);
        self.open_until_ms.store(0, Ordering::Release);
    }

    /// Failure signal for the relay channel, where a sustained outage (no client
    /// could fulfil the request) collapses to a single coarse error.
    pub fn record_ban(&self) {
        self.trip();
    }

    fn trip(&self) {
        let n = self.consecutive_bans.fetch_add(1, Ordering::AcqRel) + 1;
        if n >= BAN_THRESHOLD {
            self.open_until_ms
                .store(now_ms() + COOLDOWN_MS, Ordering::Release);
        }
    }
}

/// Hedge: run `primary`; if it hasn't succeeded within `delay`, also run `backup`
/// and take the first success. Both fail → the last error.
pub async fn hedge<T, P, B>(primary: P, delay: Duration, backup: B) -> AppResult<T>
where
    P: Future<Output = AppResult<T>>,
    B: Future<Output = AppResult<T>>,
{
    tokio::pin!(primary);
    match tokio::time::timeout(delay, &mut primary).await {
        Ok(Ok(v)) => return Ok(v),
        Ok(Err(_)) => return backup.await,
        Err(_) => {}
    }
    first_success(primary, backup).await
}

/// Race: both fired together, first success wins; both fail → the last error.
pub async fn race<T, P, B>(primary: P, backup: B) -> AppResult<T>
where
    P: Future<Output = AppResult<T>>,
    B: Future<Output = AppResult<T>>,
{
    tokio::pin!(primary);
    first_success(primary, backup).await
}

async fn first_success<T, P, B>(mut primary: std::pin::Pin<&mut P>, backup: B) -> AppResult<T>
where
    P: Future<Output = AppResult<T>>,
    B: Future<Output = AppResult<T>>,
{
    tokio::pin!(backup);
    // Each arm is disabled once its channel has resolved, so the select never sees
    // both arms disabled; when both have errored we return.
    let mut perr: Option<AppError> = None;
    let mut berr: Option<AppError> = None;
    loop {
        tokio::select! {
            r = &mut primary, if perr.is_none() => match r {
                Ok(v) => return Ok(v),
                Err(e) => perr = Some(e),
            },
            r = &mut backup, if berr.is_none() => match r {
                Ok(v) => return Ok(v),
                Err(e) => berr = Some(e),
            },
        }
        if perr.is_some() && berr.is_some() {
            return Err(berr
                .take()
                .or_else(|| perr.take())
                .unwrap_or_else(|| AppError::internal("orchestration: both channels failed")));
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn breaker_trips_after_threshold_and_resets_on_ok() {
        let h = ChannelHealth::default();
        for _ in 0..BAN_THRESHOLD - 1 {
            h.record_ban();
            assert!(!h.is_open());
        }
        h.record_ban();
        assert!(h.is_open(), "breaker must open at the threshold");
        h.record_ok();
        assert!(!h.is_open(), "a success must reset the breaker");
    }

    #[tokio::test]
    async fn hedge_returns_primary_when_fast() {
        let r = hedge(
            async { Ok(Value::from("p")) },
            Duration::from_millis(50),
            async { Ok(Value::from("b")) },
        )
        .await;
        assert_eq!(r.unwrap(), Value::from("p"));
    }

    #[tokio::test]
    async fn hedge_falls_back_when_primary_fails_fast() {
        let r = hedge(
            async { Err(AppError::internal("x")) },
            Duration::from_millis(50),
            async { Ok(Value::from("b")) },
        )
        .await;
        assert_eq!(r.unwrap(), Value::from("b"));
    }

    #[tokio::test]
    async fn hedge_backup_wins_when_primary_slow() {
        let r = hedge(
            async {
                tokio::time::sleep(Duration::from_millis(500)).await;
                Ok(Value::from("p"))
            },
            Duration::from_millis(20),
            async { Ok(Value::from("b")) },
        )
        .await;
        assert_eq!(r.unwrap(), Value::from("b"));
    }

    #[tokio::test]
    async fn race_first_success_wins() {
        let r = race(
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(Value::from("p"))
            },
            async { Ok(Value::from("b")) },
        )
        .await;
        assert_eq!(r.unwrap(), Value::from("b"));
    }

    #[tokio::test]
    async fn race_both_fail_returns_err() {
        let r: AppResult<Value> = race(async { Err(AppError::internal("p")) }, async {
            Err(AppError::internal("b"))
        })
        .await;
        assert!(r.is_err());
    }
}
