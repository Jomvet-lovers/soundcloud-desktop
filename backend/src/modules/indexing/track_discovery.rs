use std::sync::{Arc, Weak};
use std::time::Duration;

use bytes::Bytes;
use mini_moka::sync::Cache;
use once_cell::sync::Lazy;
use regex::Regex;
use tokio::sync::Mutex as AsyncMutex;
use tracing::debug;

use crate::modules::indexing::IndexingService;
use crate::modules::tracks::TrackPriority;
use crate::sc::{ScClient, TrackObserver};

static URN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"soundcloud:tracks:(\d+)").unwrap());

const TTL: Duration = Duration::from_secs(5 * 60);
const SEEN_CAPACITY: u64 = 20_000;
const INFLIGHT_CAPACITY: u64 = 4096;
const INFLIGHT_TTL: Duration = Duration::from_secs(2 * 60);
const MAX_BODY_SCAN_BYTES: usize = 512 * 1024;

pub struct TrackDiscoveryService {
    sc: ScClient,
    indexing: Arc<IndexingService>,
    recently_seen: Cache<String, ()>,
    inflight: Cache<String, Arc<AsyncMutex<()>>>,
    weak_self: Weak<Self>,
}

impl TrackDiscoveryService {
    pub fn new(sc: ScClient, indexing: Arc<IndexingService>) -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            sc,
            indexing,
            recently_seen: Cache::builder()
                .max_capacity(SEEN_CAPACITY)
                .time_to_idle(TTL)
                .build(),
            inflight: Cache::builder()
                .max_capacity(INFLIGHT_CAPACITY)
                .time_to_idle(INFLIGHT_TTL)
                .build(),
            weak_self: weak.clone(),
        })
    }

    fn lock_for(&self, sc_track_id: &str) -> Arc<AsyncMutex<()>> {
        if let Some(l) = self.inflight.get(&sc_track_id.to_string()) {
            return l;
        }
        let l = Arc::new(AsyncMutex::new(()));
        self.inflight.insert(sc_track_id.to_string(), l.clone());
        l
    }

    async fn run_one(self: Arc<Self>, sc_track_id: String, access_token: String) {
        let lock = self.lock_for(&sc_track_id);
        let _g = lock.lock().await;

        match self
            .sc
            .api_get_value(
                &format!("/tracks/soundcloud:tracks:{sc_track_id}"),
                &access_token,
                None,
            )
            .await
        {
            Ok(track) => {
                if let Err(e) = self
                    .indexing
                    .ingest_track_from_sc(&track, TrackPriority::Discovery)
                    .await
                {
                    debug!(track = %sc_track_id, error = %e, "ingest_track_from_sc failed");
                }
            }
            Err(e) => {
                debug!(track = %sc_track_id, error = %e, "discovery fetch failed");
            }
        }
    }
}

impl TrackObserver for TrackDiscoveryService {
    fn observe(&self, body: Bytes, access_token: String) {
        if access_token.is_empty() || body.is_empty() {
            return;
        }
        let scan_len = body.len().min(MAX_BODY_SCAN_BYTES);
        let snippet = match std::str::from_utf8(&body[..scan_len]) {
            Ok(s) => s,
            Err(_) => return,
        };

        let mut fresh: Vec<String> = Vec::new();
        for caps in URN_RE.captures_iter(snippet) {
            let id = caps[1].to_string();
            if self.recently_seen.contains_key(&id) {
                continue;
            }
            self.recently_seen.insert(id.clone(), ());
            fresh.push(id);
        }
        if fresh.is_empty() {
            return;
        }

        let Some(svc_arc) = self.weak_self.upgrade() else {
            return;
        };
        for id in fresh {
            let svc = svc_arc.clone();
            let token = access_token.clone();
            tokio::spawn(async move { svc.run_one(id, token).await });
        }
    }
}
