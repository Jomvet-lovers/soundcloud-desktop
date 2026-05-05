use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::bus::nats::NatsService;
use crate::bus::subjects::{streams, subjects};
use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::AppResult;
use crate::modules::lyrics::LyricsService;
use crate::modules::transcode::TranscodeTriggerService;

const REAP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const REAP_AGE: Duration = Duration::from_secs(5 * 60);
const REAP_BATCH: i64 = 50;

#[derive(Debug, Clone, Serialize)]
pub struct IndexingStats {
    pub indexed: i64,
    pub pending: i64,
}

pub struct IndexingService {
    pg: PgPool,
    nats: Arc<NatsService>,
    lyrics: Arc<LyricsService>,
    trigger: Arc<TranscodeTriggerService>,
}

impl IndexingService {
    pub fn new(
        pg: PgPool,
        nats: Arc<NatsService>,
        lyrics: Arc<LyricsService>,
        trigger: Arc<TranscodeTriggerService>,
    ) -> Arc<Self> {
        Arc::new(Self { pg, nats, lyrics, trigger })
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        self.subscribe_done();
        self.subscribe_storage_uploaded();
        self.spawn_reap_loop(shutdown);
    }

    pub async fn ensure_track_indexed(self: &Arc<Self>, sc_track: &Value) -> AppResult<()> {
        let urn = sc_track.get("urn").and_then(|v| v.as_str()).unwrap_or("");
        if urn.is_empty() {
            return Ok(());
        }
        let sc_track_id = match urn.rsplit_once(':') {
            Some((_, id)) => id.to_string(),
            None => urn.to_string(),
        };

        let existing: Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            "SELECT id, indexed_at FROM indexed_tracks WHERE sc_track_id = $1",
        )
        .bind(&sc_track_id)
        .fetch_optional(&self.pg)
        .await?;
        if let Some((_, Some(_))) = existing {
            return Ok(());
        }

        if existing.is_none() {
            let title = sc_track.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let genre = sc_track.get("genre").and_then(|v| v.as_str());
            let tag_list = sc_track.get("tag_list").and_then(|v| v.as_str()).unwrap_or("");
            let tags: Vec<String> = tag_list
                .split_whitespace()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let duration_ms = sc_track.get("duration").and_then(|v| v.as_i64()).map(|v| v as i32);
            let artwork_url = sc_track.get("artwork_url").and_then(|v| v.as_str());
            let stream_url = sc_track.get("stream_url").and_then(|v| v.as_str());

            let inserted: Option<(Uuid,)> = sqlx::query_as(
                "INSERT INTO indexed_tracks (sc_track_id, title, genre, tags, duration_ms, artwork_url, stream_url, raw_sc_data, indexed_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL) \
                 ON CONFLICT (sc_track_id) DO NOTHING \
                 RETURNING id",
            )
            .bind(&sc_track_id)
            .bind(title)
            .bind(genre)
            .bind(&tags)
            .bind(duration_ms)
            .bind(artwork_url)
            .bind(stream_url)
            .bind(sc_track)
            .fetch_optional(&self.pg)
            .await?;
            if inserted.is_none() {
                return Ok(());
            }
        }

        self.trigger.trigger(&sc_track_id);
        let lyrics = self.lyrics.clone();
        let id = sc_track_id.clone();
        tokio::spawn(async move {
            if let Err(e) = lyrics.ensure_lyrics_for_indexing(&id).await {
                debug!(track = %id, error = %e, "ensureLyricsForIndexing failed");
            }
        });
        Ok(())
    }

    pub async fn ensure_tracks_indexed(self: &Arc<Self>, tracks: &[Value]) {
        for t in tracks {
            let svc = self.clone();
            let track = t.clone();
            tokio::spawn(async move {
                if let Err(e) = svc.ensure_track_indexed(&track).await {
                    debug!(error = %e, "ensureTrackIndexed failed");
                }
            });
        }
    }

    pub async fn ensure_track_queued_by_id(&self, sc_track_id: &str) -> AppResult<()> {
        let row: Option<(String, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            "SELECT sc_track_id, indexed_at FROM indexed_tracks WHERE sc_track_id = $1",
        )
        .bind(sc_track_id)
        .fetch_optional(&self.pg)
        .await?;
        let Some((id, indexed_at)) = row else {
            warn!(sc_track_id, "Cannot queue: not in indexed_tracks");
            return Ok(());
        };
        if indexed_at.is_some() {
            return Ok(());
        }
        self.trigger.trigger(&id);
        Ok(())
    }

    pub async fn get_stats(&self) -> AppResult<IndexingStats> {
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*)::int8 FROM indexed_tracks")
            .fetch_one(&self.pg)
            .await?;
        let indexed: (i64,) = sqlx::query_as(
            "SELECT COUNT(*)::int8 FROM indexed_tracks WHERE indexed_at IS NOT NULL",
        )
        .fetch_one(&self.pg)
        .await?;
        Ok(IndexingStats {
            indexed: indexed.0,
            pending: total.0 - indexed.0,
        })
    }

    fn subscribe_storage_uploaded(self: &Arc<Self>) {
        let svc = self.clone();
        self.nats.consume(
            streams::STORAGE_EVENTS.name,
            "backend-storage-uploaded",
            Some(subjects::STORAGE_TRACK_UPLOADED),
            move |data, _meta| {
                let svc = svc.clone();
                async move {
                    let sc_track_id_raw = data.get("sc_track_id").and_then(|v| v.as_str()).unwrap_or("");
                    let storage_url = data
                        .get("storage_url")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_default();
                    let Some(sc_track_id) = normalize_sc_track_id(sc_track_id_raw) else {
                        return Ok(());
                    };
                    if storage_url.is_empty() {
                        return Ok(());
                    }

                    let existing: Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
                        "SELECT id, indexed_at FROM indexed_tracks WHERE sc_track_id = $1",
                    )
                    .bind(&sc_track_id)
                    .fetch_optional(&svc.pg)
                    .await?;

                    if let Some((_, Some(_))) = existing {
                        sqlx::query(
                            "UPDATE indexed_tracks SET s3_verified_at = now(), s3_missing_at = NULL \
                             WHERE sc_track_id = $1",
                        )
                        .bind(&sc_track_id)
                        .execute(&svc.pg)
                        .await?;
                        return Ok(());
                    }

                    if existing.is_none() {
                        let inserted: Option<(Uuid,)> = sqlx::query_as(
                            "INSERT INTO indexed_tracks (sc_track_id, indexed_at, s3_verified_at, s3_missing_at) \
                             VALUES ($1, NULL, now(), NULL) \
                             ON CONFLICT (sc_track_id) DO NOTHING \
                             RETURNING id",
                        )
                        .bind(&sc_track_id)
                        .fetch_optional(&svc.pg)
                        .await?;
                        if inserted.is_none() {
                            let post: Option<(Option<chrono::DateTime<chrono::Utc>>,)> =
                                sqlx::query_as(
                                    "SELECT indexed_at FROM indexed_tracks WHERE sc_track_id = $1",
                                )
                                .bind(&sc_track_id)
                                .fetch_optional(&svc.pg)
                                .await?;
                            if matches!(post, Some((Some(_),))) {
                                sqlx::query(
                                    "UPDATE indexed_tracks SET s3_verified_at = now(), s3_missing_at = NULL WHERE sc_track_id = $1",
                                )
                                .bind(&sc_track_id)
                                .execute(&svc.pg)
                                .await?;
                                return Ok(());
                            }
                        }
                    } else {
                        sqlx::query(
                            "UPDATE indexed_tracks SET s3_verified_at = now(), s3_missing_at = NULL WHERE sc_track_id = $1",
                        )
                        .bind(&sc_track_id)
                        .execute(&svc.pg)
                        .await?;
                    }

                    svc.nats
                        .publish(
                            subjects::INDEX_AUDIO,
                            &json!({ "sc_track_id": sc_track_id, "s3_url": storage_url }),
                        )
                        .await?;
                    info!(track = %sc_track_id, "[storage→index] published to NATS");
                    let lyrics = svc.lyrics.clone();
                    let id = sc_track_id.clone();
                    let url = storage_url;
                    tokio::spawn(async move {
                        lyrics.handle_uploaded(&id, &url).await;
                    });
                    Ok(())
                }
            },
        );
    }

    fn subscribe_done(self: &Arc<Self>) {
        let svc = self.clone();
        self.nats.consume(
            streams::DONE.name,
            "backend-done-index-audio",
            Some(subjects::DONE_INDEX_AUDIO),
            move |data, _meta| {
                let svc = svc.clone();
                async move {
                    let Some(sc_track_id) = data.get("sc_track_id").and_then(|v| v.as_str()) else {
                        return Ok(());
                    };
                    sqlx::query(
                        "UPDATE indexed_tracks SET indexed_at = now() \
                         WHERE sc_track_id = $1 AND indexed_at IS NULL",
                    )
                    .bind(sc_track_id)
                    .execute(&svc.pg)
                    .await?;
                    debug!(track = %sc_track_id, "indexed_at set");
                    Ok(())
                }
            },
        );
    }

    fn spawn_reap_loop(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(REAP_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = svc.reap().await {
                            warn!(error = %e, "indexing reap failed");
                        }
                    }
                }
            }
        });
    }

    async fn reap(self: &Arc<Self>) -> AppResult<()> {
        let cutoff =
            chrono::Utc::now().naive_utc() - chrono::Duration::from_std(REAP_AGE).unwrap();
        let stuck: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM indexed_tracks \
             WHERE indexed_at IS NULL AND created_at < $1 LIMIT $2",
        )
        .bind(cutoff)
        .bind(REAP_BATCH)
        .fetch_all(&self.pg)
        .await?;
        if stuck.is_empty() {
            return Ok(());
        }
        info!(count = stuck.len(), "indexing reap: retriggering stuck tracks");
        for (id,) in stuck {
            self.trigger.trigger(&id);
        }
        Ok(())
    }
}
