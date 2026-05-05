use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, warn};

use crate::bus::nats::NatsService;
use crate::bus::subjects::subjects;
use crate::config::LtrCfg;
use crate::error::AppResult;

pub const LTR_FEATURE_COUNT: usize = 6;

const RPC_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize)]
pub struct LtrExample {
    pub group: u64,
    pub label: i32,
    pub features: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct ScoreResp {
    scores: Option<Vec<f32>>,
    #[serde(default)]
    fallback: Option<bool>,
}

pub struct LtrService {
    nats: Arc<NatsService>,
    cfg: LtrCfg,
}

impl LtrService {
    pub fn new(nats: Arc<NatsService>, cfg: LtrCfg) -> Arc<Self> {
        Arc::new(Self { nats, cfg })
    }

    pub fn enabled(&self) -> bool {
        self.cfg.rerank_enabled
    }

    pub async fn score(&self, features: &[Vec<f32>]) -> Option<Vec<f32>> {
        if !self.enabled() || features.is_empty() {
            return None;
        }
        let resp: Option<ScoreResp> = match self
            .nats
            .request(
                subjects::AI_LTR_SCORE,
                &json!({ "features": features }),
                RPC_TIMEOUT,
                false,
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                debug!(error = %e, "ltr.score failed");
                return None;
            }
        };
        let scores = resp.and_then(|r| {
            if r.fallback.unwrap_or(false) {
                debug!("ltr.score using fallback (no trained model on worker)");
            }
            r.scores
        })?;
        if scores.len() != features.len() {
            warn!(
                got = scores.len(),
                want = features.len(),
                "ltr.score length mismatch"
            );
            return None;
        }
        Some(scores)
    }

    pub async fn publish_training(&self, examples: &[LtrExample]) -> AppResult<()> {
        if examples.is_empty() {
            return Ok(());
        }
        self.nats
            .publish(subjects::TRAIN_LTR, &json!({ "examples": examples }))
            .await
    }
}
