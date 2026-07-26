use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub struct Topology {
    pub meta: Meta,
    #[serde(default)]
    pub ingest: Vec<IngestSink>,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    #[serde(default)]
    pub workers: Workers,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Meta {
    pub version: u32,
    #[serde(default = "default_probe_interval")]
    pub probe_interval_secs: u64,
}

fn default_probe_interval() -> u64 {
    300
}

#[derive(Clone, Debug, Deserialize)]
pub struct IngestSink {
    pub tier: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
}

impl IngestSink {
    pub fn origin(&self) -> Option<String> {
        let raw = self.url.as_deref().or(self.target.as_deref())?;
        let parsed = url::Url::parse(raw).ok()?;
        let host = parsed.host_str()?;
        let mut origin = format!("{}://{host}", parsed.scheme());
        if let Some(port) = parsed.port() {
            origin.push_str(&format!(":{port}"));
        }
        Some(origin)
    }

    pub fn is_worker(&self) -> bool {
        self.tier == "worker"
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub host: String,
    pub direct: String,
    #[serde(default)]
    pub relay: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Workers {
    #[serde(default)]
    pub bases: Vec<String>,
    #[serde(default)]
    pub no_worker: Vec<String>,
}

impl Workers {
    pub fn applies_to(&self, endpoint_id: &str) -> bool {
        !self.bases.is_empty() && !self.no_worker.iter().any(|id| id == endpoint_id)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    pub endpoint: String,
    pub host: String,
    pub tier: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub err_kind: Option<String>,
}

impl Topology {
    /// Built-in route only bootstraps discovery/reporting. The live server
    /// topology replaces it before the first probe whenever any ingest path works.
    pub fn bootstrap() -> Self {
        let sink = |tier: &str, url: &str| IngestSink {
            tier: tier.to_string(),
            url: Some(url.to_string()),
            target: None,
        };
        let endpoint = |id: &str, host: &str, direct: &str, relay: &str| Endpoint {
            id: id.to_string(),
            host: host.to_string(),
            direct: direct.to_string(),
            relay: Some(relay.to_string()),
        };

        Self {
            meta: Meta {
                version: 0,
                probe_interval_secs: default_probe_interval(),
            },
            ingest: vec![
                sink("direct", "https://health.scdinternal.site/report"),
                sink("direct", "https://health-star.scdinternal.site/report"),
                sink("relay", "https://health.temp.scdinternal.site/report"),
                sink("relay", "https://health-star.temp.scdinternal.site/report"),
            ],
            endpoints: vec![
                endpoint(
                    "api",
                    "main",
                    "https://api.scdinternal.site/health",
                    "https://api.temp.scdinternal.site/health",
                ),
                endpoint(
                    "stream",
                    "main",
                    "https://stream.scdinternal.site/health",
                    "https://stream.temp.scdinternal.site/health",
                ),
                endpoint(
                    "storage",
                    "main",
                    "https://storage.scdinternal.site/health",
                    "https://storage.temp.scdinternal.site/health",
                ),
                endpoint(
                    "images",
                    "main",
                    "https://images.scdinternal.site/health",
                    "https://images.temp.scdinternal.site/health",
                ),
                endpoint(
                    "pay",
                    "main",
                    "https://pay.scdinternal.site/health",
                    "https://pay.temp.scdinternal.site/health",
                ),
            ],
            workers: Workers::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Topology;

    #[test]
    fn bootstrap_can_reach_both_health_nodes_through_temp() {
        let topology = Topology::bootstrap();
        assert_eq!(topology.meta.probe_interval_secs, 300);
        assert!(topology.ingest.iter().any(|sink| {
            sink.url.as_deref() == Some("https://health.temp.scdinternal.site/report")
        }));
        assert!(topology.ingest.iter().any(|sink| {
            sink.url.as_deref() == Some("https://health-star.temp.scdinternal.site/report")
        }));
    }
}
