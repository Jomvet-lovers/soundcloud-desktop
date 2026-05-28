use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::bus::nats::NatsService;
use crate::bus::subjects;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct RankCandidate {
    pub idx: usize,
    pub source: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RankResult {
    pub best_idx: usize,
    pub score: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LangResult {
    pub language: String,
    pub confidence: f32,
}

pub struct WorkerClient {
    nats: Arc<NatsService>,
}

impl WorkerClient {
    pub fn new(nats: Arc<NatsService>) -> Arc<Self> {
        Arc::new(Self { nats })
    }

    pub async fn detect_language(&self, text: &str) -> AppResult<Option<LangResult>> {
        self.nats
            .request::<_, LangResult>(
                subjects::AI_DETECT_LANGUAGE,
                &serde_json::json!({ "text": text }),
                Duration::from_secs(15),
                false,
            )
            .await
    }

    pub async fn generate_search_queries(
        &self,
        artist: &str,
        title: &str,
    ) -> AppResult<Vec<String>> {
        #[derive(Deserialize)]
        struct Resp {
            queries: Option<Vec<String>>,
        }
        let res: Option<Resp> = self
            .nats
            .request(
                subjects::AI_SEARCH_QUERIES,
                &serde_json::json!({ "artist": artist, "title": title }),
                Duration::from_secs(40),
                false,
            )
            .await?;
        let queries: Vec<String> = res
            .and_then(|r| r.queries)
            .map(|v| v.into_iter().filter(|q| !q.trim().is_empty()).collect())
            .unwrap_or_default();
        if queries.is_empty() {
            let fallback = format!("{artist} {title}").trim().to_string();
            if fallback.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![fallback])
            }
        } else {
            Ok(queries)
        }
    }

    pub async fn rank_lyrics(
        &self,
        artist: &str,
        title: &str,
        candidates: &[RankCandidate],
    ) -> AppResult<Option<RankResult>> {
        if candidates.is_empty() {
            return Ok(None);
        }
        self.nats
            .request(
                subjects::AI_RANK_LYRICS,
                &serde_json::json!({
                    "artist": artist,
                    "title": title,
                    "candidates": candidates,
                }),
                Duration::from_secs(60),
                false,
            )
            .await
    }

    pub async fn encode_text_mulan(&self, text: &str) -> AppResult<Option<Vec<f32>>> {
        #[derive(Deserialize)]
        struct Resp {
            vector: Option<Vec<f32>>,
        }
        let res: Option<Resp> = self
            .nats
            .request(
                subjects::AI_ENCODE_TEXT_MULAN,
                &serde_json::json!({ "text": text }),
                Duration::from_secs(15),
                false,
            )
            .await?;
        Ok(res.and_then(|r| r.vector))
    }

    pub async fn score_quality(&self, features: &[Vec<f32>]) -> AppResult<Option<Vec<f32>>> {
        if features.is_empty() {
            return Ok(Some(Vec::new()));
        }
        #[derive(Deserialize)]
        struct Resp {
            scores: Option<Vec<f32>>,
        }
        let res: Option<Resp> = self
            .nats
            .request(
                subjects::AI_QUALITY_SCORE,
                &serde_json::json!({ "features": features }),
                Duration::from_secs(10),
                false,
            )
            .await?;
        Ok(res.and_then(|r| r.scores))
    }
}
