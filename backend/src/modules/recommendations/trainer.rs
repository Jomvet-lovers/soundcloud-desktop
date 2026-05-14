use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use tracing::{info, warn};

use crate::bus::nats::NatsService;
use crate::bus::subjects::subjects;
use crate::error::AppResult;
use crate::qdrant::{collections, QdrantService};

use super::quality_features::{build_features, load_track_meta, vec_stats};
use super::service::RecommendationsService;

const MIN_TWO_TOWER_EXAMPLES: usize = 200;
const MIN_SEQUENTIAL_SESSIONS: usize = 20;
const MIN_QUALITY_EXAMPLES: usize = 100;
const ENGAGEMENT_WINDOW_HOURS: i64 = 4;
const TWO_TOWER_FEATURE_LEN: usize = 8;
const TWO_TOWER_LIMIT: i64 = 50_000;
const SESSION_GAP_MIN: i64 = 30;
const SESSION_LOOKBACK_DAYS: i64 = 30;
const SESSION_MAX: i64 = 500;
const SESSION_MIN_LEN: usize = 5;
const QUALITY_POS_PLAYS: i64 = 1_000;
const QUALITY_POS_LIKES: i64 = 50;
const QUALITY_NEG_LIMIT: i64 = 500;

#[derive(Debug, Serialize)]
struct TwoTowerExample {
    features: Vec<f32>,
    label: f32,
}

#[derive(Debug, Serialize)]
struct TwoTowerPayload {
    examples: Vec<TwoTowerExample>,
    epochs: usize,
}

#[derive(Debug, Serialize)]
struct SequentialPayload {
    sessions: Vec<Vec<Vec<f32>>>,
    epochs: usize,
}

#[derive(Debug, Serialize)]
struct QualityExample {
    features: Vec<f32>,
    label: f32,
}

#[derive(Debug, Serialize)]
struct QualityPayload {
    examples: Vec<QualityExample>,
}

pub async fn kick_off_two_tower(pg: &PgPool, nats: Arc<NatsService>) -> AppResult<usize> {
    let examples = build_two_tower_dataset(pg).await?;
    let n = examples.len();
    if n < MIN_TWO_TOWER_EXAMPLES {
        info!(n, "two_tower: dataset too small");
        return Ok(0);
    }
    nats.publish(
        subjects::TRAIN_TWO_TOWER,
        &TwoTowerPayload {
            examples,
            epochs: 30,
        },
    )
    .await?;
    info!(n, "two_tower: training kicked off");
    Ok(n)
}

pub async fn kick_off_sequential(
    pg: &PgPool,
    qdrant: Arc<QdrantService>,
    nats: Arc<NatsService>,
) -> AppResult<usize> {
    let sessions = build_sequential_dataset(pg, &qdrant).await?;
    let n = sessions.len();
    if n < MIN_SEQUENTIAL_SESSIONS {
        info!(n, "sequential: dataset too small");
        return Ok(0);
    }
    nats.publish(
        subjects::TRAIN_SEQUENTIAL,
        &SequentialPayload {
            sessions,
            epochs: 10,
        },
    )
    .await?;
    info!(n, "sequential: training kicked off");
    Ok(n)
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

async fn build_two_tower_dataset(pg: &PgPool) -> AppResult<Vec<TwoTowerExample>> {
    // Single query: each impression LEFT JOIN'd to the first matching engagement
    // event in the [shown_at, shown_at + window] window via a LATERAL subquery.
    // Avoids the N+1 of one SELECT per impression.
    let rows: Vec<(Option<Value>, Option<f32>, Option<String>)> = sqlx::query_as(
        "SELECT i.features, i.score, eng.event_type
         FROM rec_impressions i
         LEFT JOIN LATERAL (
             SELECT ue.event_type
             FROM user_events ue
             WHERE ue.sc_user_id = i.sc_user_id
               AND ue.sc_track_id = i.sc_track_id
               AND ue.created_at >= i.shown_at
               AND ue.created_at <= i.shown_at + make_interval(hours => $2::int)
             ORDER BY ue.created_at
             LIMIT 1
         ) eng ON true
         WHERE i.shown_at > NOW() - INTERVAL '7 days'
           AND i.features IS NOT NULL
         LIMIT $1",
    )
    .bind(TWO_TOWER_LIMIT)
    .bind(ENGAGEMENT_WINDOW_HOURS)
    .fetch_all(pg)
    .await?;

    let mut examples = Vec::with_capacity(rows.len());
    for (features_json, score, event_type) in rows {
        let mut features = parse_features(&features_json).unwrap_or_default();
        if features.len() != TWO_TOWER_FEATURE_LEN {
            features.resize(TWO_TOWER_FEATURE_LEN, 0.0);
            if let Some(s) = score {
                features[0] = s;
            }
        }
        let label: f32 = match event_type.as_deref() {
            Some("like" | "playlist_add" | "full_play") => 1.0,
            _ => 0.0,
        };
        examples.push(TwoTowerExample { features, label });
    }
    Ok(examples)
}

async fn build_sequential_dataset(
    pg: &PgPool,
    qdrant: &QdrantService,
) -> AppResult<Vec<Vec<Vec<f32>>>> {
    let session_rows: Vec<(Vec<String>,)> = sqlx::query_as(
        "WITH events AS (
             SELECT sc_user_id, sc_track_id, created_at,
                 EXTRACT(EPOCH FROM (
                     created_at - LAG(created_at) OVER (
                         PARTITION BY sc_user_id ORDER BY created_at
                     )
                 ))::bigint AS gap_sec
             FROM user_events
             WHERE event_type IN ('full_play', 'like', 'playlist_add')
               AND created_at > NOW() - make_interval(days => $1::int)
         ),
         sessioned AS (
             SELECT sc_user_id, sc_track_id, created_at,
                 SUM(CASE WHEN gap_sec IS NULL OR gap_sec > $2 THEN 1 ELSE 0 END)
                     OVER (PARTITION BY sc_user_id ORDER BY created_at) AS session_id
             FROM events
         )
         SELECT ARRAY_AGG(sc_track_id ORDER BY created_at) AS track_ids
         FROM sessioned
         GROUP BY sc_user_id, session_id
         HAVING COUNT(*) >= $3
         LIMIT $4",
    )
    .bind(SESSION_LOOKBACK_DAYS)
    .bind(SESSION_GAP_MIN * 60)
    .bind(SESSION_MIN_LEN as i64)
    .bind(SESSION_MAX)
    .fetch_all(pg)
    .await
    .unwrap_or_default();

    if session_rows.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_ids: Vec<u64> = Vec::new();
    let mut sessions_ids: Vec<Vec<u64>> = Vec::with_capacity(session_rows.len());
    for (track_ids,) in &session_rows {
        let parsed: Vec<u64> = track_ids
            .iter()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        for id in &parsed {
            all_ids.push(*id);
        }
        sessions_ids.push(parsed);
    }
    let vec_map = retrieve_mert_vectors(qdrant, &all_ids).await;

    let mut out: Vec<Vec<Vec<f32>>> = Vec::with_capacity(sessions_ids.len());
    for session in sessions_ids {
        let vectors: Vec<Vec<f32>> = session
            .into_iter()
            .filter_map(|n| vec_map.get(&n.to_string()).cloned())
            .collect();
        if vectors.len() >= SESSION_MIN_LEN {
            out.push(vectors);
        }
    }
    Ok(out)
}

async fn build_quality_dataset(service: &RecommendationsService) -> AppResult<Vec<QualityExample>> {
    let pos_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT it.sc_track_id
         FROM indexed_tracks it
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
         JOIN indexed_tracks it ON it.sc_track_id = d.sc_track_id
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

async fn retrieve_mert_vectors(qdrant: &QdrantService, ids: &[u64]) -> HashMap<String, Vec<f32>> {
    use qdrant_client::qdrant::{
        point_id::PointIdOptions, vector_output::Vector as VectorVariant,
        vectors_output::VectorsOptions, GetPointsBuilder, PointId,
    };
    let mut out = HashMap::new();
    if ids.is_empty() {
        return out;
    }
    let pids: Vec<PointId> = ids
        .iter()
        .map(|id| PointId {
            point_id_options: Some(PointIdOptions::Num(*id)),
        })
        .collect();
    match qdrant
        .raw()
        .get_points(GetPointsBuilder::new(collections::TRACKS_MERT, pids).with_vectors(true))
        .await
    {
        Ok(r) => {
            for p in r.result {
                let id_str = match p.id.and_then(|id| id.point_id_options) {
                    Some(PointIdOptions::Num(n)) => n.to_string(),
                    Some(PointIdOptions::Uuid(u)) => u,
                    None => continue,
                };
                if let Some(vectors) = p.vectors {
                    if let Some(VectorsOptions::Vector(v)) = vectors.vectors_options {
                        if let VectorVariant::Dense(dense) = v.into_vector() {
                            out.insert(id_str, dense.data);
                        }
                    }
                }
            }
        }
        Err(e) => warn!(error = %e, "trainer: retrieve_vectors failed"),
    }
    out
}

fn parse_features(v: &Option<Value>) -> Option<Vec<f32>> {
    let v = v.as_ref()?;
    let arr = v.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        out.push(x.as_f64().unwrap_or(0.0) as f32);
    }
    Some(out)
}
