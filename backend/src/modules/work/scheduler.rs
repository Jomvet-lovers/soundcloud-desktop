use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{mpsc, Semaphore};
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::{SchedulerPolicy, WorkOutcome, WorkSource};

const KICK_BUFFER: usize = 1024;

/// Hand-out side of the in-process kick channel. `kick(key)` nudges a single id
/// to be processed ASAP, replacing the external NATS hop for the fresh-ingest
/// fast path. Best-effort: dropped when the buffer is full (the priority-ordered
/// batch claim is the safety net). Cloneable.
#[derive(Clone)]
pub struct Kicker {
    tx: mpsc::Sender<String>,
}

impl Kicker {
    pub fn kick(&self, key: impl Into<String>) {
        let _ = self.tx.try_send(key.into());
    }
}

/// Drive a `WorkSource` with a bounded worker pool. The loop is permit-gated
/// (backpressure) and drains the source continuously: it refills an in-memory
/// buffer from the DB only when empty, so sustained throughput is
/// concurrency/latency-bound, not tick-bound. When the source is dry it idles
/// until the next tick or kick. Returns a `Kicker` for the fast path.
pub fn spawn<S: WorkSource>(
    source: Arc<S>,
    policy: SchedulerPolicy,
    shutdown: CancellationToken,
) -> Kicker {
    let (tx, mut rx) = mpsc::channel::<String>(KICK_BUFFER);
    let sem = Arc::new(Semaphore::new(policy.concurrency.max(1)));
    let lease = policy.lease_timeout;
    let batch = policy.batch.max(1);

    tokio::spawn(async move {
        let mut ticker = interval(policy.tick);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut buffer: VecDeque<S::Item> = VecDeque::new();
        info!(
            source = policy.name,
            concurrency = policy.concurrency,
            "work scheduler started"
        );

        loop {
            if shutdown.is_cancelled() {
                break;
            }
            // Backpressure: never have more than `concurrency` items in flight.
            let permit = match sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => break,
            };

            if buffer.is_empty() {
                match source.claim(batch, lease).await {
                    Ok(items) => {
                        if !items.is_empty() {
                            debug!(source = policy.name, count = items.len(), "claimed");
                        }
                        buffer.extend(items);
                    }
                    Err(e) => warn!(source = policy.name, error = %e, "claim failed"),
                }
            }

            if let Some(item) = buffer.pop_front() {
                dispatch(source.clone(), permit, item);
                continue;
            }

            // Source dry: release the permit and idle until tick or kick.
            drop(permit);
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = ticker.tick() => {}
                key = rx.recv() => {
                    let Some(key) = key else { continue };
                    match source.claim_one(&key, lease).await {
                        Ok(Some(item)) => buffer.push_back(item),
                        Ok(None) => {}
                        Err(e) => warn!(source = policy.name, error = %e, "claim_one failed"),
                    }
                }
            }
        }
        info!(source = policy.name, "work scheduler stopped");
    });

    Kicker { tx }
}

/// Spawn the per-item run + terminal write, holding the permit until done.
fn dispatch<S: WorkSource>(
    source: Arc<S>,
    permit: tokio::sync::OwnedSemaphorePermit,
    item: S::Item,
) {
    tokio::spawn(async move {
        let _permit = permit;
        // No catch_unwind: release builds are panic=abort, so a panic in run()
        // aborts the process rather than unwinding; the lease is reclaimed by the
        // next claim once it expires, not converted to a Failed outcome here.
        let outcome = source.run(&item).await;
        let res = match &outcome {
            WorkOutcome::Done => source.on_success(&item).await,
            _ => source.on_failure(&item, &outcome).await,
        };
        if let Err(e) = res {
            warn!(source = source.name(), error = %e, "terminal write failed");
        }
    });
}
