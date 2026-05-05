use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize, Serializer};
use sqlx::types::Uuid;
use sqlx::PgPool;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ListeningHistory {
    pub id: Uuid,
    #[serde(rename = "soundcloudUserId")]
    pub soundcloud_user_id: String,
    #[serde(rename = "scTrackId")]
    pub sc_track_id: String,
    pub title: String,
    pub artist_name: String,
    pub artist_urn: Option<String>,
    pub artwork_url: Option<String>,
    pub duration: i32,
    #[serde(serialize_with = "ts_iso")]
    pub played_at: NaiveDateTime,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecordHistoryDto {
    #[serde(rename = "scTrackId")]
    pub sc_track_id: String,
    pub title: String,
    #[serde(rename = "artistName")]
    pub artist_name: String,
    #[serde(rename = "artistUrn")]
    pub artist_urn: Option<String>,
    #[serde(rename = "artworkUrl")]
    pub artwork_url: Option<String>,
    pub duration: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPage {
    pub collection: Vec<ListeningHistory>,
    pub total: i64,
}

pub struct HistoryService {
    pg: PgPool,
}

impl HistoryService {
    pub fn new(pg: PgPool) -> Arc<Self> {
        Arc::new(Self { pg })
    }

    pub async fn record(&self, sc_user_id: &str, data: &RecordHistoryDto) -> AppResult<()> {
        let cutoff = chrono::Utc::now().naive_utc() - Duration::seconds(60);
        let recent: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM listening_history \
             WHERE soundcloud_user_id = $1 AND sc_track_id = $2 AND played_at > $3 \
             ORDER BY played_at DESC LIMIT 1",
        )
        .bind(sc_user_id)
        .bind(&data.sc_track_id)
        .bind(cutoff)
        .fetch_optional(&self.pg)
        .await?;
        if recent.is_some() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO listening_history \
             (soundcloud_user_id, sc_track_id, title, artist_name, artist_urn, artwork_url, duration) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(sc_user_id)
        .bind(&data.sc_track_id)
        .bind(&data.title)
        .bind(&data.artist_name)
        .bind(&data.artist_urn)
        .bind(&data.artwork_url)
        .bind(data.duration)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    pub async fn find_all(
        &self,
        sc_user_id: &str,
        limit: i64,
        offset: i64,
    ) -> AppResult<HistoryPage> {
        let collection: Vec<ListeningHistory> = sqlx::query_as(
            "SELECT * FROM listening_history WHERE soundcloud_user_id = $1 \
             ORDER BY played_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(sc_user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pg)
        .await?;
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM listening_history WHERE soundcloud_user_id = $1",
        )
        .bind(sc_user_id)
        .fetch_one(&self.pg)
        .await?;
        Ok(HistoryPage {
            collection,
            total: total.0,
        })
    }

    pub async fn clear(&self, sc_user_id: &str) -> AppResult<()> {
        sqlx::query("DELETE FROM listening_history WHERE soundcloud_user_id = $1")
            .bind(sc_user_id)
            .execute(&self.pg)
            .await?;
        Ok(())
    }
}

pub fn ts_iso<S: Serializer>(dt: &NaiveDateTime, s: S) -> Result<S::Ok, S::Error> {
    let utc = DateTime::<Utc>::from_naive_utc_and_offset(*dt, Utc);
    s.serialize_str(&utc.to_rfc3339_opts(SecondsFormat::Millis, true))
}
