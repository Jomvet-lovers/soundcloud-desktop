use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::{AppError, AppResult};
use crate::modules::events::EventsService;

pub struct DislikesService {
    pg: PgPool,
    events: Arc<EventsService>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DislikesPage {
    pub collection: Vec<Value>,
    pub next_href: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusResult {
    pub status: String,
}

impl DislikesService {
    pub fn new(pg: PgPool, events: Arc<EventsService>) -> Arc<Self> {
        Arc::new(Self { pg, events })
    }

    pub async fn add(
        &self,
        sc_user_id: &str,
        sc_track_id: &str,
        track_data: Option<&Value>,
    ) -> AppResult<StatusResult> {
        let Some(id) = normalize_sc_track_id(sc_track_id) else {
            return Ok(StatusResult {
                status: "invalid".into(),
            });
        };

        let inserted: Option<(uuid::Uuid,)> = sqlx::query_as(
            "INSERT INTO disliked_tracks (sc_user_id, sc_track_id, track_data) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (sc_user_id, sc_track_id) DO NOTHING \
             RETURNING id",
        )
        .bind(sc_user_id)
        .bind(&id)
        .bind(track_data)
        .fetch_optional(&self.pg)
        .await?;

        if inserted.is_some() {
            self.events.record(sc_user_id, &id, "dislike", None).await?;
        }
        Ok(StatusResult {
            status: "ok".into(),
        })
    }

    pub async fn remove(&self, sc_user_id: &str, sc_track_id: &str) -> AppResult<StatusResult> {
        let Some(id) = normalize_sc_track_id(sc_track_id) else {
            return Ok(StatusResult {
                status: "invalid".into(),
            });
        };
        sqlx::query("DELETE FROM disliked_tracks WHERE sc_user_id = $1 AND sc_track_id = $2")
            .bind(sc_user_id)
            .bind(&id)
            .execute(&self.pg)
            .await?;
        Ok(StatusResult {
            status: "removed".into(),
        })
    }

    pub async fn is_disliked(&self, sc_user_id: &str, sc_track_id: &str) -> AppResult<bool> {
        let Some(id) = normalize_sc_track_id(sc_track_id) else {
            return Ok(false);
        };
        self.is_disliked_by_user_id(sc_user_id, &id).await
    }

    pub async fn is_disliked_by_user_id(
        &self,
        sc_user_id: &str,
        sc_track_id: &str,
    ) -> AppResult<bool> {
        let Some(id) = normalize_sc_track_id(sc_track_id) else {
            return Ok(false);
        };
        let row: Option<(uuid::Uuid,)> = sqlx::query_as(
            "SELECT id FROM disliked_tracks \
             WHERE sc_user_id = $1 AND sc_track_id = $2 LIMIT 1",
        )
        .bind(sc_user_id)
        .bind(&id)
        .fetch_optional(&self.pg)
        .await?;
        Ok(row.is_some())
    }

    pub async fn list_ids_by_user_id(
        &self,
        sc_user_id: &str,
        limit: i64,
    ) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM disliked_tracks \
             WHERE sc_user_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(sc_user_id)
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn find_all(
        &self,
        sc_user_id: &str,
        limit: i64,
        cursor: Option<&str>,
    ) -> AppResult<DislikesPage> {
        let cursor_dt = match cursor {
            Some(s) => Some(parse_cursor(s)?),
            None => None,
        };

        let rows: Vec<(Option<Value>, NaiveDateTime)> = if let Some(dt) = cursor_dt {
            sqlx::query_as(
                "SELECT track_data, created_at FROM disliked_tracks \
                 WHERE sc_user_id = $1 AND created_at < $2 \
                 ORDER BY created_at DESC LIMIT $3",
            )
            .bind(sc_user_id)
            .bind(dt)
            .bind(limit + 1)
            .fetch_all(&self.pg)
            .await?
        } else {
            sqlx::query_as(
                "SELECT track_data, created_at FROM disliked_tracks \
                 WHERE sc_user_id = $1 \
                 ORDER BY created_at DESC LIMIT $2",
            )
            .bind(sc_user_id)
            .bind(limit + 1)
            .fetch_all(&self.pg)
            .await?
        };

        let has_more = rows.len() as i64 > limit;
        let slice: Vec<(Option<Value>, NaiveDateTime)> =
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
        let collection: Vec<Value> = slice.into_iter().filter_map(|(td, _)| td).collect();
        Ok(DislikesPage {
            collection,
            next_href,
        })
    }
}

fn parse_cursor(s: &str) -> AppResult<NaiveDateTime> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.naive_utc())
        .map_err(|e| AppError::bad_request(format!("invalid cursor: {e}")))
}
