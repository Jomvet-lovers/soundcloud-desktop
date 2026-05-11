use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

pub struct Throttle {
    interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Arc<Self> {
        Arc::new(Self {
            interval,
            last: Mutex::new(None),
        })
    }

    pub async fn wait(&self) {
        let mut g = self.last.lock().await;
        if let Some(t) = *g {
            let elapsed = t.elapsed();
            if elapsed < self.interval {
                tokio::time::sleep(self.interval - elapsed).await;
            }
        }
        *g = Some(Instant::now());
    }
}
