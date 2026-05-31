//! Generic Postgres-claim worker-pool runtime. Generalizes the proven
//! sync_queue pattern (claim-with-lease + backoff + give-up) so enrich and all
//! catalog discovery run as `WorkSource` instances over one substrate.
//!
//! Invariants the substrate guarantees:
//! - attempts increment at CLAIM (same UPDATE that sets the lease) → a claimed
//!   row instantly has a positive backoff window; the attempts=0 republish loop
//!   cannot be expressed.
//! - a dedicated lease column self-expires on crash (cutoff in SQL via DB clock)
//!   → no reaper, in-flight/success/failure are distinguishable.
//! - an explicit terminal state at the attempt cap → the working set shrinks
//!   monotonically.
//!
//! No pooled DB connection is held across `run()` (external I/O); claim and the
//! per-item terminal write are separate short transactions.

mod scheduler;

use std::future::Future;
use std::time::Duration;

use chrono::Utc;

use crate::error::AppResult;

pub use scheduler::{spawn, Kicker};

/// Result of a single unit of work. The source decides the SQL; the runtime
/// only routes Done → on_success, anything else → on_failure.
#[derive(Debug)]
pub enum WorkOutcome {
    Done,
    /// Ban / rate-limit / budget exhausted: NOT the item's fault. The source
    /// rolls back the claim-time attempt increment and backs off by `backoff`.
    ExternalBlocked { backoff: Duration },
    /// Real failure. Burns the claim-time attempt; terminal at the source cap.
    Failed { error: String },
}

/// Runtime knobs. Backoff math and the attempt cap live in the source
/// (per-table columns), not here — this is pure scheduling.
#[derive(Clone, Debug)]
pub struct SchedulerPolicy {
    pub name: &'static str,
    pub concurrency: usize,
    pub batch: i64,
    pub tick: Duration,
    pub lease_timeout: Duration,
}

/// A claimable work domain. Implemented per table (tracks/artists/wanted_tracks).
/// Methods return `impl Future + Send` (RPITIT) so the generic `spawn` can drive
/// them on tokio without `async-trait`.
pub trait WorkSource: Send + Sync + 'static {
    type Item: Send + Sync + 'static;

    fn name(&self) -> &'static str;

    /// Atomically lease + attempts++ a batch of due rows, return the items.
    /// `lease_timeout` is used in-SQL (`now() - interval`) so a crashed worker's
    /// lease self-expires on the DB clock.
    fn claim(
        &self,
        batch: i64,
        lease_timeout: Duration,
    ) -> impl Future<Output=AppResult<Vec<Self::Item>>> + Send;

    /// Targeted claim for the in-process kick channel (low-latency first touch).
    /// Sources without a fast path return `Ok(None)`.
    fn claim_one(
        &self,
        key: &str,
        lease_timeout: Duration,
    ) -> impl Future<Output=AppResult<Option<Self::Item>>> + Send;

    /// The only place external I/O happens. Holds no pooled connection.
    fn run(&self, item: &Self::Item) -> impl Future<Output=WorkOutcome> + Send;

    /// Terminal success write: set done, clear lease, reset attempts/fail-count.
    fn on_success(&self, item: &Self::Item) -> impl Future<Output=AppResult<()>> + Send;

    /// Failure write: backoff via next_run_at (or terminal at cap), clear lease.
    fn on_failure(
        &self,
        item: &Self::Item,
        outcome: &WorkOutcome,
    ) -> impl Future<Output=AppResult<()>> + Send;
}

/// Centralized backoff: `min(base * 2^attempts, cap)`. Called by every source's
/// on_failure so the curve is identical everywhere (mirrors sync_queue).
pub fn backoff_after(attempts: i32, base: Duration, cap: Duration) -> Duration {
    let shift = attempts.clamp(0, 16) as u32;
    let secs = base
        .as_secs()
        .max(1)
        .saturating_mul(1u64 << shift)
        .min(cap.as_secs().max(1));
    Duration::from_secs(secs)
}

/// `now() + backoff` as a chrono timestamp, for binding into next_run_at writes.
pub fn next_run_after(attempts: i32, base: Duration, cap: Duration) -> chrono::DateTime<Utc> {
    let d = backoff_after(attempts, base, cap);
    Utc::now() + chrono::Duration::from_std(d).unwrap_or_else(|_| chrono::Duration::seconds(60))
}
