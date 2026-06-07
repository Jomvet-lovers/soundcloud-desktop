use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Cap distinct route keys to keep the map bounded even if path normalization
/// misses something exotic.
const MAX_ROUTES: usize = 500;

#[derive(Default, Clone, serde::Serialize)]
pub struct RouteStat {
    pub count: u64,
    pub total_ms: u64,
    pub max_ms: u64,
    pub errors: u64,
}

/// Process-wide HTTP request counters, populated by the tracking middleware.
pub struct HttpMetrics {
    started: Instant,
    routes: Mutex<HashMap<String, RouteStat>>,
}

impl Default for HttpMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpMetrics {
    pub fn new() -> Self {
        Self { started: Instant::now(), routes: Mutex::new(HashMap::new()) }
    }

    pub fn record(&self, key: &str, ms: u64, status: u16) {
        let mut m = self.routes.lock().unwrap();
        if let Some(e) = m.get_mut(key) {
            e.count += 1;
            e.total_ms += ms;
            if ms > e.max_ms {
                e.max_ms = ms;
            }
            if status >= 500 {
                e.errors += 1;
            }
        } else if m.len() < MAX_ROUTES {
            m.insert(
                key.to_string(),
                RouteStat { count: 1, total_ms: ms, max_ms: ms, errors: (status >= 500) as u64 },
            );
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub fn snapshot(&self) -> Vec<(String, RouteStat)> {
        self.routes
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// Collapse high-cardinality path segments (UUIDs, numeric/long-hex ids) into
/// `:id` so per-route counters stay stable without axum MatchedPath.
pub fn normalize_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for seg in path.split('/') {
        if seg.is_empty() {
            continue;
        }
        out.push('/');
        if is_id_like(seg) {
            out.push_str(":id");
        } else {
            out.push_str(seg);
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

fn is_id_like(seg: &str) -> bool {
    // all-digits
    if seg.len() >= 2 && seg.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    // uuid (36 chars, 4 dashes)
    if seg.len() == 36 && seg.bytes().filter(|b| *b == b'-').count() == 4 {
        return true;
    }
    // long hex blob
    if seg.len() >= 20 && seg.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    false
}
