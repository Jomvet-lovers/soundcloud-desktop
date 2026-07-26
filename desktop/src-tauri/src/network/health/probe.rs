use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::stream::{self, StreamExt};
use reqwest::Client;

use super::model::{Endpoint, Sample, Topology, Workers};
use crate::network::edge::{self, Tier};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PARALLEL_ENDPOINTS: usize = 4;

pub async fn probe_all(client: &Client, topology: &Topology) -> Vec<Sample> {
    let workers = topology.workers.clone();
    let batches = stream::iter(topology.endpoints.clone())
        .map(|endpoint| {
            let client = client.clone();
            let workers = workers.clone();
            async move { probe_endpoint(&client, &endpoint, &workers).await }
        })
        .buffer_unordered(MAX_PARALLEL_ENDPOINTS)
        .collect::<Vec<_>>()
        .await;

    batches.into_iter().flatten().collect()
}

async fn probe_endpoint(client: &Client, endpoint: &Endpoint, workers: &Workers) -> Vec<Sample> {
    let mut samples = Vec::new();
    // Direct and the dedicated temp relay are independent infrastructure. Probe
    // both concurrently so the status page can answer "is temp ready right now?"
    // even while the primary route is healthy. Worker remains an emergency-only
    // probe to avoid spraying every endpoint through the shared worker pool.
    let direct_fut = hit(client, &endpoint.direct, None);
    let relay_fut = async {
        match endpoint.relay.as_deref() {
            Some(relay) => Some(hit(client, relay, None).await),
            None => None,
        }
    };
    let (direct, relay) = tokio::join!(direct_fut, relay_fut);

    samples.push(sample(endpoint, "direct", &direct));
    edge::note_url(&endpoint.direct, Tier::Direct, direct.ok);

    if let Some(outcome) = relay {
        samples.push(sample(endpoint, "relay", &outcome));
        if !direct.ok && outcome.ok {
            edge::note_url(&endpoint.direct, Tier::Relay, true);
            return samples;
        }
    }

    if direct.ok {
        return samples;
    }

    if workers.applies_to(&endpoint.id) {
        let outcome = hit_worker(client, &endpoint.direct, &workers.bases).await;
        samples.push(sample(endpoint, "worker", &outcome));
        if outcome.ok {
            edge::note_url(&endpoint.direct, Tier::Worker, true);
        }
    }

    samples
}

async fn hit(client: &Client, url: &str, x_target: Option<&str>) -> Outcome {
    let mut request = client.get(url).timeout(PROBE_TIMEOUT);
    if let Some(target) = x_target {
        request = request.header("X-Target", BASE64.encode(target.as_bytes()));
    }

    let started = Instant::now();
    match request.send().await {
        Ok(response) if response.status().is_success() || response.status().is_redirection() => {
            Outcome::ok(started.elapsed().as_millis() as i32)
        }
        Ok(_) => Outcome::error("status"),
        Err(error) if error.is_timeout() => Outcome::error("timeout"),
        Err(error) if error.is_connect() => Outcome::error("connect"),
        Err(_) => Outcome::error("request"),
    }
}

async fn hit_worker(client: &Client, target: &str, bases: &[String]) -> Outcome {
    let mut last = Outcome::error("worker");
    for base in bases {
        last = hit(client, base, Some(target)).await;
        if last.ok {
            return last;
        }
    }
    last
}

struct Outcome {
    ok: bool,
    latency_ms: Option<i32>,
    err_kind: Option<String>,
}

impl Outcome {
    fn ok(latency_ms: i32) -> Self {
        Self {
            ok: true,
            latency_ms: Some(latency_ms),
            err_kind: None,
        }
    }

    fn error(kind: &str) -> Self {
        Self {
            ok: false,
            latency_ms: None,
            err_kind: Some(kind.to_string()),
        }
    }
}

fn sample(endpoint: &Endpoint, tier: &str, outcome: &Outcome) -> Sample {
    Sample {
        endpoint: endpoint.id.clone(),
        host: endpoint.host.clone(),
        tier: tier.to_string(),
        ok: outcome.ok,
        latency_ms: outcome.latency_ms,
        err_kind: outcome.err_kind.clone(),
    }
}
