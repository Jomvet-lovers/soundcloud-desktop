use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveMode {
    Similar,
    Diverse,
}

impl WaveMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("diverse") => Self::Diverse,
            _ => Self::Similar,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendResult {
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(rename = "playbackCount", skip_serializing_if = "Option::is_none")]
    pub playback_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SeedVectors {
    pub collab: Option<Vec<f32>>,
    pub mert: Option<Vec<f32>>,
    pub clap: Option<Vec<f32>>,
    pub lyrics: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScoredCandidate {
    pub id: u64,
    pub score: f32,
    pub payload: Option<HashMap<String, Value>>,
    pub features: Vec<f32>,
}
