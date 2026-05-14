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
        Arc::new(Self {
            pg,
            nats,
            lyrics,
            trigger,
        })
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

        let existing: Option<(Uuid, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as("SELECT id, indexed_at FROM indexed_tracks WHERE sc_track_id = $1")
                .bind(&sc_track_id)
                .fetch_optional(&self.pg)
                .await?;
        if let Some((_, Some(_))) = existing {
            return Ok(());
        }

        if existing.is_none() {
            let title = sc_track.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let genre = sc_track.get("genre").and_then(|v| v.as_str());
            let tag_list = sc_track
                .get("tag_list")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tags: Vec<String> = tag_list
                .split_whitespace()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let duration_ms = sc_track
                .get("duration")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);
            let artwork_url = sc_track.get("artwork_url").and_then(|v| v.as_str());
            let stream_url = sc_track.get("stream_url").and_then(|v| v.as_str());
            let uploader_sc_user_id = extract_uploader_sc_user_id(sc_track);
            let (release_year, release_date) = crate::common::release_date::extract(sc_track);

            let inserted: Option<(Uuid,)> = sqlx::query_as(
                "INSERT INTO indexed_tracks (sc_track_id, title, genre, tags, duration_ms, artwork_url, stream_url, raw_sc_data, uploader_sc_user_id, release_year, release_date, indexed_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULL) \
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
            .bind(uploader_sc_user_id.as_deref())
            .bind(release_year)
            .bind(release_date)
            .fetch_optional(&self.pg)
            .await?;
            if inserted.is_none() {
                return Ok(());
            }
        }

        self.trigger.trigger(&sc_track_id);
        if let Err(e) = crate::modules::enrich::publish_enrich(&self.nats, &sc_track_id).await {
            debug!(track = %sc_track_id, error = %e, "enrich publish failed");
        }
        let lyrics = self.lyrics.clone();
        let id = sc_track_id.clone();
        tokio::spawn(async move {
            if let Err(e) = lyrics.ensure_lyrics_for_indexing(&id).await {
                debug!(track = %id, error = %e, "ensureLyricsForIndexing failed");
            }
        });
        Ok(())
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
            move |data| {
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
            move |data| {
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
                    if let Some(fp) = data.get("fingerprint").and_then(|v| v.as_str()) {
                        if !fp.is_empty() {
                            svc.apply_fingerprint(sc_track_id, fp).await?;
                        }
                    }
                    Ok(())
                }
            },
        );
    }

    async fn apply_fingerprint(
        self: &Arc<Self>,
        sc_track_id: &str,
        fingerprint: &str,
    ) -> AppResult<()> {
        let row: Option<(uuid::Uuid, Option<uuid::Uuid>)> = sqlx::query_as(
            "SELECT id, canonical_track_id FROM indexed_tracks WHERE sc_track_id = $1",
        )
        .bind(sc_track_id)
        .fetch_optional(&self.pg)
        .await?;
        let Some((track_id, canonical)) = row else {
            return Ok(());
        };

        sqlx::query("UPDATE indexed_tracks SET audio_fingerprint = $2 WHERE id = $1")
            .bind(track_id)
            .bind(fingerprint)
            .execute(&self.pg)
            .await?;

        let prefix: String = fingerprint.chars().take(64).collect();
        let neighbour: Option<(uuid::Uuid, Option<uuid::Uuid>)> = sqlx::query_as(
            "SELECT id, canonical_track_id FROM indexed_tracks
             WHERE substr(audio_fingerprint, 1, 64) = $1
               AND id <> $2
             LIMIT 1",
        )
        .bind(&prefix)
        .bind(track_id)
        .fetch_optional(&self.pg)
        .await?;
        let Some((other_id, other_canonical)) = neighbour else {
            return Ok(());
        };

        let canonical_id = canonical
            .or(other_canonical)
            .unwrap_or_else(uuid::Uuid::new_v4);
        sqlx::query(
            "UPDATE indexed_tracks SET canonical_track_id = $1
             WHERE id IN ($2, $3) AND (canonical_track_id IS NULL OR canonical_track_id <> $1)",
        )
        .bind(canonical_id)
        .bind(track_id)
        .bind(other_id)
        .execute(&self.pg)
        .await?;
        debug!(track = %sc_track_id, %canonical_id, "fingerprint canonicalized");
        Ok(())
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
        let cutoff = chrono::Utc::now().naive_utc() - chrono::Duration::from_std(REAP_AGE).unwrap();
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
        info!(
            count = stuck.len(),
            "indexing reap: retriggering stuck tracks"
        );
        for (id,) in stuck {
            self.trigger.trigger(&id);
        }
        Ok(())
    }
}

fn extract_uploader_sc_user_id(sc_track: &Value) -> Option<String> {
    let user = sc_track.get("user")?;
    if let Some(id) = user.get("id") {
        if let Some(s) = id.as_str() {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
        if let Some(n) = id.as_i64() {
            return Some(n.to_string());
        }
    }
    if let Some(urn) = user.get("urn").and_then(|v| v.as_str()) {
        let tail = urn.rsplit(':').next().unwrap_or("");
        if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            return Some(tail.to_string());
        }
    }
    None
}
