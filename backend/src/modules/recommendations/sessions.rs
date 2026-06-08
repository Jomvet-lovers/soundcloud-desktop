use chrono::{DateTime, Datelike, Timelike, Utc};
use sqlx::PgPool;

use crate::error::AppResult;
use crate::modules::centroids::normalize;
use crate::qdrant::collections;

use super::service::RecommendationsService;

const SESSION_WINDOW_HOURS: i64 = 2;
const SESSION_MIN_TRACKS: usize = 5;
const HOUR_WINDOW: u32 = 1;
const HOUR_LOOKBACK_WEEKS: i64 = 4;

pub struct SessionContext {
    pub centroid: Vec<f32>,
}

pub struct HourContext {
    pub centroid: Vec<f32>,
}

impl RecommendationsService {
    pub async fn detect_current_session(
        &self,
        sc_user_id: &str,
    ) -> AppResult<Option<SessionContext>> {
        let ids = recent_played_ids(&self.pg, sc_user_id, SESSION_WINDOW_HOURS, 80).await?;
        if ids.len() < SESSION_MIN_TRACKS {
            return Ok(None);
        }
        let numeric: Vec<u64> = ids.iter().filter_map(|s| s.parse::<u64>().ok()).collect();
        if numeric.is_empty() {
            return Ok(None);
        }
        let vec_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric)
            .await;
        let mut points: Vec<Vec<f32>> = numeric
            .iter()
            .filter_map(|n| vec_map.get(&n.to_string()).cloned())
            .collect();
        if points.len() < SESSION_MIN_TRACKS {
            return Ok(None);
        }
        let dim = points[0].len();
        let mut acc = vec![0f32; dim];
        let count = points.len() as f32;
        for v in points.drain(..) {
            for (i, x) in v.into_iter().enumerate() {
                if i < dim {
                    acc[i] += x;
                }
            }
        }
        for x in acc.iter_mut() {
            *x /= count;
        }
        normalize(&mut acc);
        Ok(Some(SessionContext { centroid: acc }))
    }

    pub async fn hour_context(
        &self,
        sc_user_id: &str,
        now: DateTime<Utc>,
    ) -> AppResult<Option<HourContext>> {
        let hour = now.hour();
        let dow = now.weekday().num_days_from_monday() as i32;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT sc_track_id FROM user_events
             WHERE sc_user_id = ANY($1)
               AND event_type IN ('full_play', 'like', 'playlist_add')
               AND created_at > NOW() - make_interval(weeks => $2::int)
               AND ABS(EXTRACT(HOUR FROM created_at)::int - $3) <= $4
               AND EXTRACT(DOW FROM created_at)::int = $5
             ORDER BY created_at DESC
             LIMIT 60",
        )
            .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
        .bind(HOUR_LOOKBACK_WEEKS)
        .bind(hour as i32)
        .bind(HOUR_WINDOW as i32)
        .bind(dow)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();
        if ids.len() < 5 {
            return Ok(None);
        }
        let numeric: Vec<u64> = ids.iter().filter_map(|s| s.parse::<u64>().ok()).collect();
        if numeric.is_empty() {
            return Ok(None);
        }
        let vec_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric)
            .await;
        let mut acc: Option<Vec<f32>> = None;
        let mut count = 0usize;
        for n in &numeric {
            if let Some(v) = vec_map.get(&n.to_string()) {
                match acc.as_mut() {
                    Some(a) => {
                        let dim = a.len().min(v.len());
                        for i in 0..dim {
                            a[i] += v[i];
                        }
                    }
                    None => acc = Some(v.clone()),
                }
                count += 1;
            }
        }
        let mut a = match acc {
            Some(a) => a,
            None => return Ok(None),
        };
        if count == 0 {
            return Ok(None);
        }
        let inv = 1.0 / count as f32;
        for x in a.iter_mut() {
            *x *= inv;
        }
        normalize(&mut a);
        Ok(Some(HourContext { centroid: a }))
    }
}

async fn recent_played_ids(
    pg: &PgPool,
    sc_user_id: &str,
    hours: i64,
    limit: i64,
) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id FROM user_events
         WHERE sc_user_id = ANY($1)
           AND event_type IN ('full_play', 'like', 'playlist_add')
           AND created_at > NOW() - make_interval(hours => $2::int)
         ORDER BY created_at DESC
         LIMIT $3",
    )
        .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
    .bind(hours)
    .bind(limit)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub fn mix_centroids(
    base: Option<&[f32]>,
    session: Option<&[f32]>,
    hour: Option<&[f32]>,
) -> Option<Vec<f32>> {
    const W_BASE: f32 = 0.6;
    const W_SESSION: f32 = 0.25;
    const W_HOUR: f32 = 0.15;

    let any = base.is_some() || session.is_some() || hour.is_some();
    if !any {
        return None;
    }
    let dim = base
        .map(|v| v.len())
        .or_else(|| session.map(|v| v.len()))
        .or_else(|| hour.map(|v| v.len()))?;
    let mut acc = vec![0f32; dim];
    let mut total_w = 0f32;
    let mut add = |v: Option<&[f32]>, w: f32| {
        if let Some(v) = v {
            let n = dim.min(v.len());
            for i in 0..n {
                acc[i] += v[i] * w;
            }
            total_w += w;
        }
    };
    add(base, W_BASE);
    add(session, W_SESSION);
    add(hour, W_HOUR);
    if total_w <= 0.0 {
        return None;
    }
    for x in acc.iter_mut() {
        *x /= total_w;
    }
    normalize(&mut acc);
    Some(acc)
}
