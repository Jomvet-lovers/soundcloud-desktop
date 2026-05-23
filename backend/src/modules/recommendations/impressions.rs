use std::collections::HashMap;

use serde_json::Value;
use sqlx::PgPool;
use tracing::warn;

use super::clusters::Cluster;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpressionSource {
    Home,
    Similar,
    Artist,
}

impl ImpressionSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Similar => "similar",
            Self::Artist => "artist",
        }
    }
}

pub fn log_clusters_async(
    pg: PgPool,
    sc_user_id: String,
    source: ImpressionSource,
    clusters: &[Cluster],
    features_map: &HashMap<String, Vec<f32>>,
) {
    if sc_user_id.is_empty() || clusters.is_empty() {
        return;
    }
    struct Row {
        user: String,
        track: String,
        cluster: String,
        position: i16,
        features: Option<Value>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for c in clusters {
        for (pos, id) in c.track_ids.iter().enumerate() {
            let features = features_map.get(id).map(|v| Value::from(v.clone()));
            rows.push(Row {
                user: sc_user_id.clone(),
                track: id.clone(),
                cluster: c.id.to_string(),
                position: pos as i16,
                features,
            });
        }
    }
    if rows.is_empty() {
        return;
    }
    let source_str = source.as_str();
    tokio::spawn(async move {
        let user_ids: Vec<String> = rows.iter().map(|r| r.user.clone()).collect();
        let track_ids: Vec<String> = rows.iter().map(|r| r.track.clone()).collect();
        let cluster_ids: Vec<String> = rows.iter().map(|r| r.cluster.clone()).collect();
        let positions: Vec<i16> = rows.iter().map(|r| r.position).collect();
        let sources: Vec<&str> = rows.iter().map(|_| source_str).collect();
        let features_arr: Vec<Option<Value>> = rows.iter().map(|r| r.features.clone()).collect();
        if let Err(e) = sqlx::query(
            "INSERT INTO rec_impressions
                 (sc_user_id, sc_track_id, cluster_id, source, position, features)
             SELECT * FROM UNNEST(
                 $1::text[], $2::text[], $3::text[], $4::varchar[], $5::int2[], $6::jsonb[]
             )",
        )
        .bind(&user_ids)
        .bind(&track_ids)
        .bind(&cluster_ids)
        .bind(&sources)
        .bind(&positions)
        .bind(&features_arr)
        .execute(&pg)
        .await
        {
            warn!(error = %e, "impressions: insert failed");
        }
    });
}
