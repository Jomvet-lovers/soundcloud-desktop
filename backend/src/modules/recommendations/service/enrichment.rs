use qdrant_client::qdrant::{Condition, Filter};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use crate::error::AppResult;
use crate::modules::ltr::LTR_FEATURE_COUNT;

use super::types::{RecommendResult, ScoredCandidate};
use super::util::value_id_to_string;
use super::RecommendationsService;

impl RecommendationsService {
    pub(crate) async fn enrich_and_boost(
        &self,
        items: Vec<ScoredCandidate>,
        user_languages: Option<&[String]>,
    ) -> AppResult<Vec<RecommendResult>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = items.iter().map(|it| it.id.to_string()).collect();
        let tracks: Vec<(String, Option<Value>, Option<String>)> = sqlx::query_as(
            "SELECT sc_track_id, raw_sc_data, language FROM indexed_tracks \
             WHERE sc_track_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pg)
        .await?;
        let by_id: HashMap<String, (Option<Value>, Option<String>)> = tracks
            .into_iter()
            .map(|(id, raw, lang)| (id, (raw, lang)))
            .collect();
        let boost = self.cfg.popularity_boost as f32;
        let user_lang_set: HashSet<String> = user_languages
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default();

        let mut out: Vec<RecommendResult> = items
            .into_iter()
            .map(|it| {
                let key = it.id.to_string();
                let entry = by_id.get(&key);
                let raw = entry
                    .and_then(|(r, _)| r.as_ref())
                    .cloned()
                    .unwrap_or(Value::Null);
                let language = entry.and_then(|(_, l)| l.clone());
                let artist_pub = raw
                    .get("publisher_metadata")
                    .and_then(|v| v.get("artist"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let artist_user = raw
                    .get("user")
                    .and_then(|v| v.get("username"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let artist = artist_pub.or(artist_user);
                let genre = raw.get("genre").and_then(|v| v.as_str()).map(String::from);
                let playback_count = raw
                    .get("playback_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let bonus = ((playback_count.max(0) as f64).ln_1p() as f32) * boost;
                let mut features = it.features.clone();
                if features.len() >= LTR_FEATURE_COUNT {
                    features[4] = (playback_count.max(0) as f64).ln_1p() as f32;
                    features[5] = match language.as_deref() {
                        Some(l) if user_lang_set.contains(l) => 1.0,
                        _ => 0.0,
                    };
                }
                RecommendResult {
                    id: json!(it.id),
                    score: Some(it.score + bonus),
                    payload: it.payload,
                    artist,
                    genre,
                    playback_count: Some(playback_count),
                    features: Some(features),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }

    pub(crate) fn artist_cap(
        &self,
        items: Vec<RecommendResult>,
        cap: usize,
    ) -> Vec<RecommendResult> {
        if cap == 0 {
            return items;
        }
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            let key = it
                .artist
                .clone()
                .unwrap_or_else(|| value_id_to_string(&it.id))
                .to_lowercase();
            let n = counts.get(&key).copied().unwrap_or(0);
            if n >= cap {
                continue;
            }
            counts.insert(key, n + 1);
            out.push(it);
        }
        out
    }

    pub(crate) fn build_filter(
        &self,
        exclude: &[String],
        languages: Option<&[String]>,
    ) -> Option<Filter> {
        let mut filter = Filter::default();
        let mut populated = false;

        if !exclude.is_empty() {
            let must_not: Vec<Condition> = exclude
                .iter()
                .map(|id| Condition::matches("sc_track_id", id.clone()))
                .collect();
            filter.must_not = must_not;
            populated = true;
        }
        if let Some(langs) = languages {
            if !langs.is_empty() {
                let must = vec![Condition::matches("language", langs.to_vec())];
                filter.must = must;
                populated = true;
            }
        }
        if populated {
            Some(filter)
        } else {
            None
        }
    }
}
