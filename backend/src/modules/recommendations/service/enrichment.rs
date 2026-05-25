use qdrant_client::qdrant::{Condition, Filter};
use serde_json::json;
use std::collections::{HashMap, HashSet};

use crate::error::AppResult;

use super::types::{RecommendResult, ScoredCandidate};
use super::util::value_id_to_string;
use super::RecommendationsService;

/// Длина вектора фичей в impressions. Совпадает с тем, что писали при живом
/// LTR-пайплайне; держим стабильным, чтобы аналитика по rec_impressions не
/// сломалась. См. docs/ltr-future-graph-features.md перед изменением.
const IMPRESSION_FEATURE_LEN: usize = 8;

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
        // Берём normalised поля из `tracks` (artist берём из uploader_username,
        // т.к. publisher_metadata/artist у нас уже нет: эту инфу теперь
        // выводит enrich-pipeline через track_artists; для denorm-минимума
        // достаточно uploader_username).
        let tracks: Vec<(String, Option<String>, Option<String>, Option<String>, Option<i64>)> =
            sqlx::query_as(
                "SELECT sc_track_id, uploader_username, genre, language, play_count_sc \
                 FROM tracks WHERE sc_track_id = ANY($1)",
            )
            .bind(&ids)
            .fetch_all(&self.pg)
            .await?;
        let by_id: HashMap<String, (Option<String>, Option<String>, Option<String>, Option<i64>)> =
            tracks
                .into_iter()
                .map(|(id, uploader, genre, lang, plays)| (id, (uploader, genre, lang, plays)))
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
                let artist = entry.and_then(|(u, _, _, _)| u.clone());
                let genre = entry.and_then(|(_, g, _, _)| g.clone());
                let language = entry.and_then(|(_, _, l, _)| l.clone());
                let playback_count = entry.and_then(|(_, _, _, p)| *p).unwrap_or(0);
                let bonus = ((playback_count.max(0) as f64).ln_1p() as f32) * boost;
                let mut features = it.features.clone();
                if features.len() >= IMPRESSION_FEATURE_LEN {
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
