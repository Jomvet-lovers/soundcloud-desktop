use std::sync::Arc;

use sqlx::PgPool;
use tracing::{info, warn};

use crate::error::AppResult;
use crate::qdrant::collections;

use super::quality_features::{
    build_features, fallback_score, load_track_meta, vec_stats, QUALITY_FEATURE_LEN,
};
use super::service::RecommendationsService;

const BATCH: i64 = 100;

pub async fn backfill_missing_scores(service: Arc<RecommendationsService>) -> AppResult<usize> {
    let ids: Vec<String> = sqlx::query_file_scalar!(
        "queries/recommendations/quality_scorer/select_missing_scores.sql",
        BATCH
    )
    .fetch_all(&service.pg)
    .await?;

    if ids.is_empty() {
        return Ok(0);
    }
    let meta = load_track_meta(&service.pg, &ids).await;

    let numeric_ids: Vec<u64> = ids.iter().filter_map(|s| s.parse::<u64>().ok()).collect();
    let mert_map = service
        .retrieve_vectors(collections::TRACKS_MERT, &numeric_ids)
        .await;
    let clap_map = service
        .retrieve_vectors(collections::TRACKS_CLAP, &numeric_ids)
        .await;

    let mut features_batch: Vec<Vec<f32>> = Vec::with_capacity(ids.len());
    let mut id_order: Vec<String> = Vec::with_capacity(ids.len());
    for id in &ids {
        let Some(m) = meta.get(id) else { continue };
        features_batch.push(build_features(
            m,
            vec_stats(mert_map.get(id)),
            vec_stats(clap_map.get(id)),
        ));
        id_order.push(id.clone());
    }
    if features_batch.is_empty() {
        return Ok(0);
    }

    let scores = match service.worker.score_quality(&features_batch).await {
        Ok(Some(s)) if s.len() == features_batch.len() => s,
        Ok(_) => {
            warn!("quality_scorer: worker returned no scores, fallback to heuristic");
            features_batch.iter().map(|f| fallback_score(f)).collect()
        }
        Err(e) => {
            warn!(error = %e, "quality_scorer: worker call failed");
            features_batch.iter().map(|f| fallback_score(f)).collect()
        }
    };

    persist_scores(&service.pg, &id_order, &scores).await?;
    info!(
        n = id_order.len(),
        feature_len = QUALITY_FEATURE_LEN,
        "quality_scorer: backfilled"
    );
    Ok(id_order.len())
}

async fn persist_scores(pg: &PgPool, ids: &[String], scores: &[f32]) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query_file!(
        "queries/recommendations/quality_scorer/persist_scores.sql",
        ids,
        scores
    )
    .execute(pg)
    .await?;
    Ok(())
}
