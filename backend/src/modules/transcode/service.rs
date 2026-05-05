use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tracing::debug;

use crate::bus::nats::NatsService;
use crate::bus::subjects::subjects;
use crate::config::AppConfig;

#[derive(Debug, Deserialize)]
struct TriggerResp {
    url: String,
    #[serde(default)]
    cached: bool,
}

pub struct TranscodeTriggerService {
    http: Client,
    config: Arc<AppConfig>,
    nats: Arc<NatsService>,
}

impl TranscodeTriggerService {
    pub fn new(http: Client, config: Arc<AppConfig>, nats: Arc<NatsService>) -> Arc<Self> {
        Arc::new(Self { http, config, nats })
    }

    pub fn trigger(self: &Arc<Self>, sc_track_id: &str) {
        let this = self.clone();
        let id = sc_track_id.to_string();
        tokio::spawn(async move {
            let url = format!(
                "{}/internal/transcode-upload/{}",
                this.config.streaming.service_url,
                urlencoding::encode(&id),
            );
            let token = &this.config.internal.token;
            let req = this
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {token}"))
                .json(&json!({}))
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            let resp = match req {
                Ok(r) => r,
                Err(e) => {
                    debug!(sc_track_id = %id, error = %e, "[trigger] http failed");
                    return;
                }
            };
            if !resp.status().is_success() {
                debug!(sc_track_id = %id, status = %resp.status(), "[trigger] non-2xx");
                return;
            }
            let parsed: TriggerResp = match resp.json().await {
                Ok(p) => p,
                Err(e) => {
                    debug!(sc_track_id = %id, error = %e, "[trigger] parse failed");
                    return;
                }
            };
            if parsed.cached {
                let payload = json!({
                    "sc_track_id": id,
                    "storage_url": parsed.url,
                });
                if let Err(e) = this
                    .nats
                    .publish(subjects::STORAGE_TRACK_UPLOADED, &payload)
                    .await
                {
                    debug!(sc_track_id = %id, error = %e, "[trigger] fanout publish failed");
                } else {
                    debug!(sc_track_id = %id, "[trigger] cached → fanned out");
                }
            }
        });
    }
}
