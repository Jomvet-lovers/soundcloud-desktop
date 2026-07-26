use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

const POLL_INTERVAL: Duration = Duration::from_secs(10);

pub async fn run(nudge: Arc<Notify>) {
    let mut previous = fingerprint();
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        interval.tick().await;
        let current = fingerprint();
        if current != previous {
            previous = current;
            nudge.notify_one();
        }
    }
}

fn fingerprint() -> u64 {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return 0;
    };
    let mut addresses = interfaces
        .into_iter()
        .filter(|interface| !interface.is_loopback())
        .map(|interface| format!("{}={}", interface.name, interface.ip()))
        .collect::<Vec<_>>();
    addresses.sort();

    let mut hasher = DefaultHasher::new();
    addresses.hash(&mut hasher);
    hasher.finish()
}
