use std::time::Duration;

use reqwest::Client;
use serde::Serialize;

use super::model::{Sample, Topology};

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_SAMPLES: usize = 256;

#[derive(Serialize)]
struct Report<'a> {
    client: &'a str,
    version: &'a str,
    topology: u32,
    samples: &'a [Sample],
}

pub struct Delivery {
    client: Client,
}

impl Delivery {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn fetch_topology(&self, topology: &Topology) -> Option<Topology> {
        for origin in &topology.ingest {
            let url = format!("{}/topology", origin.trim_end_matches('/'));
            let Ok(response) = self.client.get(&url).timeout(DELIVERY_TIMEOUT).send().await else {
                continue;
            };
            if !response.status().is_success() {
                continue;
            }
            if let Ok(fresh) = response.json::<Topology>().await {
                return Some(fresh.sanitized());
            }
        }
        None
    }

    pub async fn report(
        &self,
        topology: &Topology,
        client_id: &str,
        app_version: &str,
        samples: &[Sample],
    ) -> bool {
        if samples.is_empty() {
            return true;
        }
        let Ok(body) = serde_json::to_vec(&Report {
            client: client_id,
            version: app_version,
            topology: topology.version,
            samples: &samples[..samples.len().min(MAX_SAMPLES)],
        }) else {
            return false;
        };

        for origin in &topology.ingest {
            let url = format!("{}/report", origin.trim_end_matches('/'));
            let request = self
                .client
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone())
                .timeout(DELIVERY_TIMEOUT);
            if request
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                return true;
            }
        }
        false
    }
}
