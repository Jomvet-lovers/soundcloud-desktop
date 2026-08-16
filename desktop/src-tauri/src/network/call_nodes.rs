use std::time::Duration;

use futures_util::StreamExt;
use wreq::Client;
use sha2::{Digest, Sha256};

const PROBE_PATH: &str = "/probe";
const PROBE_BYTES: u64 = 64 * 1024;
const DIGEST_HEADER: &str = "x-probe-sha256";
const VERSION_HEADER: &str = "x-node-version";
const MAX_VERSION_LEN: usize = 32;
const DEADLINE: Duration = Duration::from_secs(20);
const STALL: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    Clear,
    Cut,
    Blackhole,
    Tamper,
    Dead,
}

impl Reach {
    pub fn usable(self) -> bool {
        self == Reach::Clear
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Reach::Clear => "clear",
            Reach::Cut => "cut",
            Reach::Blackhole => "blackhole",
            Reach::Tamper => "tamper",
            Reach::Dead => "dead",
        }
    }
}

pub fn order(device_id: &str, nodes: &[(String, f64)]) -> Vec<String> {
    let mut ranked: Vec<(f64, &String)> = nodes
        .iter()
        .filter(|(_, weight)| *weight > 0.0)
        .map(|(node, weight)| (score(device_id, node, *weight), node))
        .collect();
    ranked.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.cmp(right.1))
    });
    ranked.into_iter().map(|(_, node)| node.clone()).collect()
}

fn score(device_id: &str, node: &str, weight: f64) -> f64 {
    let digest = Sha256::digest(format!("{device_id}\u{1}{node}").as_bytes());
    let head = u64::from_le_bytes(digest[..8].try_into().unwrap_or_default());
    let unit = (head as f64 + 0.5) / (u64::MAX as f64 + 1.0);
    weight / -unit.ln()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub reach: Reach,
    pub node_version: Option<String>,
}

impl Probe {
    fn dead(reach: Reach) -> Self {
        Self {
            reach,
            node_version: None,
        }
    }

    pub fn usable(&self) -> bool {
        self.reach.usable()
    }

    pub fn version_or_unknown(&self) -> &str {
        self.node_version.as_deref().unwrap_or("unknown")
    }
}

pub async fn inspect(http: &Client, base: &str) -> Probe {
    let url = format!(
        "{}{PROBE_PATH}?bytes={PROBE_BYTES}",
        base.trim_end_matches('/')
    );
    let response = match http.get(&url).timeout(DEADLINE).send().await {
        Ok(response) if response.status().is_success() => response,
        Ok(_) => return Probe::dead(Reach::Dead),
        Err(error) if error.is_timeout() => return Probe::dead(Reach::Blackhole),
        Err(_) => return Probe::dead(Reach::Dead),
    };

    let expected = response
        .headers()
        .get(DIGEST_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase);
    let node_version = response
        .headers()
        .get(VERSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= MAX_VERSION_LEN)
        .map(str::to_string);

    let done = |reach: Reach| Probe {
        reach,
        node_version: node_version.clone(),
    };

    let mut body = response.bytes_stream();
    let mut digest = Sha256::new();
    let mut read: u64 = 0;
    loop {
        let next = match tokio::time::timeout(STALL, body.next()).await {
            Ok(next) => next,
            Err(_) => return done(Reach::Blackhole),
        };
        let Some(chunk) = next else { break };
        let Ok(chunk) = chunk else { return done(Reach::Cut) };
        digest.update(&chunk);
        read += chunk.len() as u64;
        if read >= PROBE_BYTES {
            break;
        }
    }

    if read < PROBE_BYTES {
        return done(Reach::Cut);
    }
    match expected {
        Some(expected) if hex(digest.finalize().as_slice()) != expected => done(Reach::Tamper),
        _ => done(Reach::Clear),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> Vec<(String, f64)> {
        vec![
            ("call-1".to_string(), 1.0),
            ("call-2".to_string(), 1.0),
            ("call-3".to_string(), 1.0),
        ]
    }

    fn spread(nodes: &[(String, f64)], devices: usize) -> Vec<usize> {
        let mut counts = vec![0usize; nodes.len()];
        for index in 0..devices {
            let chosen = order(&format!("device-{index}"), nodes);
            let at = nodes.iter().position(|(node, _)| *node == chosen[0]).unwrap();
            counts[at] += 1;
        }
        counts
    }

    #[test]
    fn the_pool_is_split_evenly_without_anyone_being_told_the_totals() {
        for count in spread(&pool(), 9_000) {
            let drift = (count as f64 - 3_000.0).abs() / 3_000.0;
            assert!(drift < 0.05, "перекос {drift}");
        }
    }

    #[test]
    fn the_same_device_always_lands_on_the_same_node() {
        assert_eq!(order("device-7", &pool()), order("device-7", &pool()));
    }

    #[test]
    fn losing_a_node_moves_only_its_own_clients() {
        let full = pool();
        let reduced: Vec<(String, f64)> =
            full.iter().filter(|(node, _)| node != "call-2").cloned().collect();
        let mut moved = 0;
        for index in 0..3_000 {
            let device = format!("device-{index}");
            let before = order(&device, &full)[0].clone();
            if before == "call-2" {
                continue;
            }
            if before != order(&device, &reduced)[0] {
                moved += 1;
            }
        }
        assert_eq!(moved, 0);
    }

    #[test]
    fn every_node_stays_in_the_order_as_a_fallback() {
        assert_eq!(order("device-1", &pool()).len(), 3);
    }

    #[test]
    fn weight_shifts_the_share_without_naming_a_single_client() {
        let weighted = vec![("call-1".to_string(), 3.0), ("call-2".to_string(), 1.0)];
        let counts = spread(&weighted, 8_000);
        assert!(counts[0] > counts[1] * 2, "{counts:?}");
    }

    #[test]
    fn a_zero_weight_node_is_drained_rather_than_used() {
        let drained = vec![("call-1".to_string(), 1.0), ("call-2".to_string(), 0.0)];
        assert_eq!(order("device-1", &drained), ["call-1"]);
    }

    #[test]
    fn only_a_whole_verified_payload_is_worth_a_long_session() {
        assert!(Reach::Clear.usable());
        assert!(!Reach::Cut.usable());
        assert!(!Reach::Blackhole.usable());
        assert!(!Reach::Tamper.usable());
    }
}
