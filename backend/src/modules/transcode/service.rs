use std::sync::Arc;
use std::time::Duration;

use mini_moka::sync::Cache;
use reqwest::Client;
use serde_json::json;
use tokio::sync::Semaphore;
use tracing::debug;

use crate::config::AppConfig;

const MAX_INFLIGHT: usize = 16;
const DEDUP_TTL: Duration = Duration::from_secs(15 * 60);
const DEDUP_CAP: u64 = 16_384;

pub struct TranscodeTriggerService {
    http: Client,
    config: Arc<AppConfig>,
    sem: Arc<Semaphore>,
    inflight: Cache<String, ()>,
}

impl TranscodeTriggerService {
    pub fn new(http: Client, config: Arc<AppConfig>) -> Arc<Self> {
        Arc::new(Self {
            http,
            config,
            sem: Arc::new(Semaphore::new(MAX_INFLIGHT)),
            inflight: Cache::builder()
                .max_capacity(DEDUP_CAP)
                .time_to_idle(DEDUP_TTL)
                .build(),
        })
    }

    /// Короткий HTTP-kick на streaming: streaming сразу 202 Accepted и фоном
    /// качает + заливает в storage. Storage по завершении сам публикует
    /// `storage.track_uploaded` в NATS — backend ловит и обновляет state.
    /// Это поэтому HTTP-timeout короткий (10s — только ack), а не 180s.
    ///
    /// Bounded concurrency — на cold-refresh-волнах ingest валит 500+ треков
    /// разом; без semaphore TCP-pool/HTTP/streaming captiously дохнут.
    /// Дедуп по sc_track_id (TTL 15 мин) глушит повторные kick'и того же
    /// трека из reap'а, пока storage-event ещё в пути.
    pub fn trigger(self: &Arc<Self>, sc_track_id: &str) {
        if self.inflight.get(&sc_track_id.to_string()).is_some() {
            return;
        }
        self.inflight.insert(sc_track_id.to_string(), ());

        let this = self.clone();
        let id = sc_track_id.to_string();
        tokio::spawn(async move {
            let _permit = match this.sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
            let urn = format!("soundcloud:tracks:{id}");
            let url = format!(
                "{}/internal/transcode-upload/{}",
                this.config.streaming.service_url,
                urlencoding::encode(&urn),
            );
            let token = &this.config.internal.token;
            let res = this
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {token}"))
                .json(&json!({}))
                .timeout(Duration::from_secs(10))
                .send()
                .await;
            match res {
                Ok(r) if r.status().is_success() => {}
                Ok(r) => {
                    debug!(sc_track_id = %id, status = %r.status(), "[trigger] non-2xx");
                    this.inflight.invalidate(&id);
                }
                Err(e) => {
                    debug!(sc_track_id = %id, error = %e, "[trigger] http failed");
                    this.inflight.invalidate(&id);
                }
            }
            // inflight НЕ инвалидируем при ack — storage публикует завершение,
            // повторный trigger из reap в ближайшие 15 минут не нужен.
        });
    }
}
