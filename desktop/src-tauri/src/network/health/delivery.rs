use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
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
            for target in targets(sink, &url, &topology.workers.bases) {
                let mut request = self.client.get(&target.url).timeout(DELIVERY_TIMEOUT);
                if let Some(value) = target.x_target {
                    request = request.header("X-Target", value);
                }
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
            for target in targets(sink, &url, &topology.workers.bases) {
                let mut request = self
                    .client
                    .post(&target.url)
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body.clone())
                    .timeout(DELIVERY_TIMEOUT);
                if let Some(value) = target.x_target {
                    request = request.header("X-Target", value);
                }
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

fn targets(sink: &IngestSink, url: &str, worker_bases: &[String]) -> Vec<Target> {
    if sink.is_worker() {
        let x_target = BASE64.encode(url.as_bytes());
        worker_bases
            .iter()
            .map(|base| Target {
                url: base.clone(),
                x_target: Some(x_target.clone()),
            })
            .collect()
    } else {
        vec![Target {
            url: url.to_string(),
            x_target: None,
        }]
    }
}

struct Target {
    url: String,
    x_target: Option<String>,
}
