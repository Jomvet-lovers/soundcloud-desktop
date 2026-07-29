use std::time::Duration;

use reqwest::Client;
use serde::Serialize;

use super::model::{IngestSink, Sample, Topology};

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Serialize)]
struct Report<'a> {
    client_id: &'a str,
    app_version: &'a str,
    topology_version: u32,
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
        for sink in &topology.ingest {
            let Some(origin) = sink.origin() else {
                continue;
            };
            let url = format!("{origin}/topology");
            for target in targets(sink, &url) {
                let request = self.client.get(&target.url).timeout(DELIVERY_TIMEOUT);
                let Ok(response) = request.send().await else {
                    continue;
                };
                if !response.status().is_success() {
                    continue;
                }
                if let Ok(fresh) = response.json::<Topology>().await {
                    return Some(fresh);
                }
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
        let Ok(body) = serde_json::to_vec(&Report {
            client_id,
            app_version,
            topology_version: topology.meta.version,
            samples,
        }) else {
            return false;
        };

        for sink in &topology.ingest {
            let Some(origin) = sink.origin() else {
                continue;
            };
            let url = format!("{origin}/report");
            for target in targets(sink, &url) {
                let request = self
                    .client
                    .post(&target.url)
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
        }
        false
    }
}

/// Воркер-тира больше нет: синк — это всегда конкретный URL.
fn targets(_sink: &IngestSink, url: &str) -> Vec<Target> {
    vec![Target {
        url: url.to_string(),
    }]
}

struct Target {
    url: String,
}
