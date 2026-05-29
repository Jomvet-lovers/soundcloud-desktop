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
        // Резервируем слот под локом (микросекунды арифметики), спим уже БЕЗ
        // лока: rate 1/interval сохраняется, но N вызовов не сериализуются на
        // самом мьютексе (каждый ждёт свой слот параллельно).
        let now = Instant::now();
        let slot = {
            let mut g = self.last.lock().await;
            let next = match *g {
                Some(t) => (t + self.interval).max(now),
                None => now,
            };
            *g = Some(next);
            next
        };
        if let Some(delay) = slot.checked_duration_since(Instant::now()) {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
        }
    }
}
