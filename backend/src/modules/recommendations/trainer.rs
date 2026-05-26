use std::sync::Arc;

use serde::Serialize;
use tracing::info;

use crate::bus::nats::NatsService;
use crate::bus::subjects;
use crate::error::AppResult;
use crate::qdrant::collections;

use super::quality_features::{build_features, load_track_meta, vec_stats};
use super::service::RecommendationsService;

const MIN_QUALITY_EXAMPLES: usize = 100;
const QUALITY_POS_PLAYS: i64 = 1_000;
const QUALITY_POS_LIKES: i64 = 50;
const QUALITY_NEG_LIMIT: i64 = 500;

#[derive(Debug, Serialize)]
struct QualityExample {
    features: Vec<f32>,
    label: f32,
}

#[derive(Debug, Serialize)]
struct QualityPayload {
    examples: Vec<QualityExample>,
}

pub async fn kick_off_quality(
    service: Arc<RecommendationsService>,
    nats: Arc<NatsService>,
) -> AppResult<usize> {
    let examples = build_quality_dataset(&service).await?;
    let n = examples.len();
    if n < MIN_QUALITY_EXAMPLES {
        info!(n, "quality: dataset too small");
        return Ok(0);
    }
    nats.publish(subjects::TRAIN_QUALITY, &QualityPayload { examples })
        .await?;
    info!(n, "quality: training kicked off");
    Ok(n)
}

async fn build_quality_dataset(service: &RecommendationsService) -> AppResult<Vec<QualityExample>> {
    let pos_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT it.sc_track_id
         FROM tracks it
         JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
         WHERE it.indexed_at IS NOT NULL
           AND COALESCE(c.play_count, 0) >= $1
           AND COALESCE(c.likes_count, 0) >= $2
         ORDER BY COALESCE(c.play_count, 0) DESC
         LIMIT 800",
    )
    .bind(QUALITY_POS_PLAYS)
    .bind(QUALITY_POS_LIKES)
    .fetch_all(&service.pg)
    .await
    .unwrap_or_default();

    let neg_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT d.sc_track_id
         FROM disliked_tracks d
         JOIN tracks it ON it.sc_track_id = d.sc_track_id
         WHERE it.indexed_at IS NOT NULL
         LIMIT $1",
    )
    .bind(QUALITY_NEG_LIMIT)
    .fetch_all(&service.pg)
    .await
    .unwrap_or_default();

    let mut entries: Vec<(String, f32)> = Vec::new();
    for (id,) in pos_rows {
        entries.push((id, 1.0));
    }
    for (id,) in neg_rows {
        entries.push((id, 0.0));
    }
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<String> = entries.iter().map(|(id, _)| id.clone()).collect();
    let meta = load_track_meta(&service.pg, &ids).await;

    let numeric_ids: Vec<u64> = ids.iter().filter_map(|s| s.parse::<u64>().ok()).collect();
    let mert_map = service
        .retrieve_vectors(collections::TRACKS_MERT, &numeric_ids)
        .await;
    let clap_map = service
        .retrieve_vectors(collections::TRACKS_CLAP, &numeric_ids)
        .await;

    let mut examples = Vec::with_capacity(entries.len());
    for (id, label) in entries {
        let m = meta.get(&id).cloned().unwrap_or_default();
        let features = build_features(
            &m,
            vec_stats(mert_map.get(&id)),
            vec_stats(clap_map.get(&id)),
        );
        examples.push(QualityExample { features, label });
    }
    Ok(examples)
}
