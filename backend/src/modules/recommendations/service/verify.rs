use serde_json::json;
use std::collections::{HashMap, HashSet};

use crate::error::AppResult;

use super::types::RecommendResult;
use super::util::value_id_to_string;
use super::RecommendationsService;

impl RecommendationsService {
    pub(crate) async fn take_verified(
        &self,
        items: Vec<RecommendResult>,
        limit: usize,
    ) -> AppResult<Vec<RecommendResult>> {
        let mut out: Vec<RecommendResult> = Vec::new();
        let batch_size = limit.max(8);
        let mut i = 0usize;
        while i < items.len() && out.len() < limit {
            let end = (i + batch_size).min(items.len());
            let slice = &items[i..end];
            let ids: Vec<String> = slice.iter().map(|s| value_id_to_string(&s.id)).collect();
            let missing = self.s3.find_missing(&ids).await?;
            for item in slice {
                if out.len() >= limit {
                    break;
                }
                if !missing.contains(&value_id_to_string(&item.id)) {
                    out.push(item.clone());
                }
            }
            i += batch_size;
        }
        Ok(out)
    }

    pub(crate) async fn get_fallback_tracks(
        &self,
        exclude: &[String],
        limit: usize,
        languages: Option<&[String]>,
    ) -> AppResult<Vec<RecommendResult>> {
        let limit_q = (limit * 3).max(60) as i64;
        let rows: Vec<(String,)> = if let Some(langs) = languages {
            if !langs.is_empty() {
                sqlx::query_as(
                    "SELECT sc_track_id FROM indexed_tracks \
                     WHERE indexed_at IS NOT NULL AND language = ANY($1) \
                     ORDER BY indexed_at DESC LIMIT $2",
                )
                .bind(langs)
                .bind(limit_q)
                .fetch_all(&self.pg)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT sc_track_id FROM indexed_tracks \
                     WHERE indexed_at IS NOT NULL \
                     ORDER BY indexed_at DESC LIMIT $1",
                )
                .bind(limit_q)
                .fetch_all(&self.pg)
                .await?
            }
        } else {
            sqlx::query_as(
                "SELECT sc_track_id FROM indexed_tracks \
                 WHERE indexed_at IS NOT NULL \
                 ORDER BY indexed_at DESC LIMIT $1",
            )
            .bind(limit_q)
            .fetch_all(&self.pg)
            .await?
        };
        let exclude_set: HashSet<String> = exclude.iter().cloned().collect();
        Ok(rows
            .into_iter()
            .filter(|(id,)| !exclude_set.contains(id))
            .take(limit)
            .map(|(id,)| {
                let mut payload = HashMap::new();
                payload.insert("sc_track_id".into(), json!(id));
                RecommendResult {
                    id: json!(id),
                    score: None,
                    payload: Some(payload),
                    artist: None,
                    genre: None,
                    playback_count: None,
                    features: None,
                }
            })
            .collect())
    }
}
