use qdrant_client::qdrant::SearchPointsBuilder;
use tracing::debug;

use crate::error::AppResult;
use crate::modules::lyrics::EncodeOutcome;
use crate::qdrant::collections;

use super::service::util::{payload_to_map, point_id_to_value, value_to_u64};
use super::service::{RecommendResult, RecommendationsService};

/// Длина LTR-features schema (исторически 8). Сейчас LTR-инференса нет, но
/// схема рассинхрона с rec_impressions ломает аналитику — держим как было.
const FEATURE_LEN: usize = 8;

/// Результат текстового поиска. `preparing` = вектор запроса ещё считается
/// воркером (см. [`EncodeOutcome::Preparing`]) — выдачи пока нет, фронт
/// показывает «готовим вайб» и переспрашивает. `failed` = транзиентный сбой
/// Qdrant (пустой результат не финальный). Ни тот, ни другой ответ кэшировать
/// нельзя — иначе пустышка залипнет на TTL.
#[derive(Debug, Default)]
pub struct SearchTextResult {
    pub preparing: bool,
    pub failed: bool,
    pub results: Vec<RecommendResult>,
}

impl RecommendationsService {
    pub async fn search_by_text(
        &self,
        query: &str,
        limit: usize,
        languages: Option<&[String]>,
    ) -> AppResult<SearchTextResult> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(SearchTextResult::default());
        }
        let vec = match self.worker.encode_text_mulan(q).await? {
            EncodeOutcome::Ready(v) if !v.is_empty() => v,
            EncodeOutcome::Preparing => {
                return Ok(SearchTextResult {
                    preparing: true,
                    failed: false,
                    results: Vec::new(),
                })
            }
            _ => return Ok(SearchTextResult::default()),
        };
        let filter = self.build_filter(&[], languages);
        let fetch_limit = (limit * 3).max(40);

        let mut builder =
            SearchPointsBuilder::new(collections::TRACKS_CLAP, vec, fetch_limit as u64)
                .with_payload(true);
        if let Some(f) = filter {
            builder = builder.filter(f);
        }
        let resp = match self.qdrant.raw().search_points(builder).await {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "searchByText: qdrant search failed");
                return Ok(SearchTextResult {
                    failed: true,
                    ..Default::default()
                });
            }
        };

        let scored: Vec<super::service::ScoredCandidate> = resp
            .result
            .into_iter()
            .filter_map(|p| {
                let id_val = point_id_to_value(p.id);
                let id = value_to_u64(&id_val)?;
                Some(super::service::ScoredCandidate {
                    id,
                    score: p.score,
                    payload: Some(payload_to_map(p.payload)),
                    features: vec![0.0; FEATURE_LEN],
                })
            })
            .collect();

        // Privacy-guard: CLAP-индекс не несёт `sharing`, режем приватные треки
        // по source-of-truth до enrichment'а.
        let public = self
            .public_track_ids(&scored.iter().map(|c| c.id.to_string()).collect::<Vec<_>>())
            .await;
        let scored: Vec<super::service::ScoredCandidate> = scored
            .into_iter()
            .filter(|c| public.contains(&c.id.to_string()))
            .collect();

        let enriched = self.enrich_and_boost(scored, languages).await?;
        let diverse = self.artist_cap(enriched, self.cfg.artist_cap);
        let results = self.take_verified(diverse, limit).await?;
        Ok(SearchTextResult {
            preparing: false,
            failed: false,
            results,
        })
    }
}
