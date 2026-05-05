use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::error::{AppError, AppResult};

pub struct LocalLikesService {
    pg: PgPool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindAllResult {
    pub collection: Vec<Value>,
    pub next_href: Option<String>,
}

impl LocalLikesService {
    pub fn new(pg: PgPool) -> Arc<Self> {
        Arc::new(Self { pg })
    }

    pub async fn add(
        &self,
        sc_user_id: &str,
        sc_track_id: &str,
        track_data: &Value,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO local_likes (soundcloud_user_id, sc_track_id, track_data) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (soundcloud_user_id, sc_track_id) DO NOTHING",
        )
        .bind(sc_user_id)
        .bind(sc_track_id)
        .bind(track_data)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    pub async fn remove(&self, sc_user_id: &str, sc_track_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM local_likes WHERE soundcloud_user_id = $1 AND sc_track_id = $2")
            .bind(sc_user_id)
            .bind(sc_track_id)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    pub async fn find_all(
        &self,
        sc_user_id: &str,
        limit: i64,
        cursor: Option<&str>,
    ) -> AppResult<FindAllResult> {
        let cursor_dt = match cursor {
            Some(s) => Some(parse_cursor(s)?),
            None => None,
        };

        let rows: Vec<(Value, NaiveDateTime)> = if let Some(dt) = cursor_dt {
            sqlx::query_as(
                "SELECT track_data, created_at FROM local_likes \
                 WHERE soundcloud_user_id = $1 AND created_at < $2 \
                 ORDER BY created_at DESC LIMIT $3",
            )
            .bind(sc_user_id)
            .bind(dt)
            .bind(limit + 1)
            .fetch_all(&self.pg)
            .await?
        } else {
            sqlx::query_as(
                "SELECT track_data, created_at FROM local_likes \
                 WHERE soundcloud_user_id = $1 \
                 ORDER BY created_at DESC LIMIT $2",
            )
            .bind(sc_user_id)
            .bind(limit + 1)
            .fetch_all(&self.pg)
            .await?
        };

        let has_more = rows.len() as i64 > limit;
        let slice: Vec<(Value, NaiveDateTime)> =
            rows.into_iter().take(limit as usize).collect();
        let next_href = if has_more {
            slice.last().map(|(_, dt)| {
                let iso = DateTime::<Utc>::from_naive_utc_and_offset(*dt, Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                format!("?limit={limit}&cursor={iso}")
            })
        } else {
            None
        };
        let collection = slice.into_iter().map(|(v, _)| v).collect();
        Ok(FindAllResult {
            collection,
            next_href,
        })
    }

    pub async fn get_liked_track_ids(
        &self,
        sc_user_id: &str,
        sc_track_ids: &[String],
    ) -> AppResult<HashSet<String>> {
        if sc_track_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM local_likes \
             WHERE soundcloud_user_id = $1 AND sc_track_id = ANY($2)",
        )
        .bind(sc_user_id)
        .bind(sc_track_ids)
        .fetch_all(&self.pg)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }
}

fn parse_cursor(s: &str) -> AppResult<NaiveDateTime> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.naive_utc())
        .map_err(|e| AppError::bad_request(format!("invalid cursor: {e}")))
}
