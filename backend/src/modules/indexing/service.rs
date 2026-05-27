use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::bus::nats::NatsService;
use crate::bus::subjects::{self, streams};
use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::AppResult;
use crate::modules::lyrics::LyricsService;
use crate::modules::tracks::normalize::ScTrackFields;
use crate::modules::tracks::{TrackPriority, TrackRepository};
use crate::modules::transcode::TranscodeTriggerService;

const REAP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const REAP_AGE: Duration = Duration::from_secs(5 * 60);
const REAP_BATCH: i64 = 50;

#[derive(Debug, Clone, Serialize)]
pub struct IndexingStats {
    pub indexed: i64,
    pub pending: i64,
}

/// IndexingService = (а) приёмная для каждого трека, который мы хотим иметь
/// в `tracks`, и (б) единая точка кикинга пайплайна транскод → S3 → qdrant.
///
/// Поток для нового трека:
/// 1. [`ingest_track_from_sc`] нормализует SC payload и UPSERT'ит строку в
///    `tracks`. Если строка только что создана → запускается пайплайн:
///    * `transcode.trigger` — заливка в S3 через streaming;
///    * `nats.publish(ENRICH_TRACK)` — поднимает artist/album linkage в
///      `enrich`-сервисе;
///    * `lyrics.ensure_lyrics_for_indexing` — поиск/прикрепление лирики.
/// 2. После заливки S3 приходит [`subjects::STORAGE_TRACK_UPLOADED`];
///    [`subscribe_storage_uploaded`] помечает storage_state и публикует
///    [`subjects::INDEX_AUDIO`] (если index_state ещё pending).
/// 3. Worker считает embedding'и и публикует [`subjects::DONE_INDEX_AUDIO`];
///    [`subscribe_done`] выставляет `tracks.indexed_at`/`index_state='indexed'`
///    и при наличии fingerprint — канонизирует дубли.
///
/// Cold-refresh новых лайков/плейлистов идёт ровно через `ingest_track_from_sc`
/// → отсюда же кикается пайплайн. После rework'а нет ни одной точки, где
/// трек попадает в БД без пайплайн-кика — это лечит регрессию, при которой
/// после перехода на cold-cache treки переставали индексироваться.
pub struct IndexingService {
    pg: PgPool,
    nats: Arc<NatsService>,
    lyrics: Arc<LyricsService>,
    trigger: Arc<TranscodeTriggerService>,
    tracks: TrackRepository,
}

impl IndexingService {
    pub fn new(
        pg: PgPool,
        nats: Arc<NatsService>,
        lyrics: Arc<LyricsService>,
        trigger: Arc<TranscodeTriggerService>,
    ) -> Arc<Self> {
        let tracks = TrackRepository::new(pg.clone());
        Arc::new(Self {
            pg,
            nats,
            lyrics,
            trigger,
            tracks,
        })
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        self.subscribe_done();
        self.subscribe_storage_uploaded();
        self.spawn_reap_loop(shutdown);
    }

    /// Принимает SC payload и проводит трек через ingest + pipeline-kick.
    /// `priority` определяет позицию в pickup-очередях индексации/storage'а
    /// (likes — раньше discovery, см. [`TrackPriority`]).
    pub async fn ingest_track_from_sc(
        self: &Arc<Self>,
        payload: &Value,
        priority: TrackPriority,
    ) -> AppResult<()> {
        let Some(fields) = ScTrackFields::from_sc(payload) else {
            debug!(
                urn = payload.get("urn").and_then(|v| v.as_str()).unwrap_or(""),
                title = payload.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "ingest skipped: ScTrackFields::from_sc returned None"
            );
            return Ok(());
        };
        let result = self
            .tracks
            .upsert_from_sc(&fields, priority, priority)
            .await?;
        if result.was_new {
            self.kick_pipeline(&fields.sc_track_id);
        }
        Ok(())
    }

    /// Перекикнуть пайплайн для существующего трека (используется events
    /// при play и reap'ом «зависших» треков).
    pub async fn trigger_indexing(&self, sc_track_id: &str) {
        self.trigger.trigger(sc_track_id);
    }

    /// Внутренний хелпер: stradge → transcode + enrich + lyrics ensure.
    /// Lyrics — в spawn'е, чтобы не блокировать caller; остальные синхронны
    /// (но дёшевы — это NATS publish и in-memory trigger).
    fn kick_pipeline(self: &Arc<Self>, sc_track_id: &str) {
        self.trigger.trigger(sc_track_id);
        let nats = self.nats.clone();
        let id_for_enrich = sc_track_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = crate::modules::enrich::publish_enrich(&nats, &id_for_enrich).await {
                debug!(track = %id_for_enrich, error = %e, "enrich publish failed");
            }
        });
        let lyrics = self.lyrics.clone();
        let id_for_lyrics = sc_track_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = lyrics.ensure_lyrics_for_indexing(&id_for_lyrics).await {
                debug!(track = %id_for_lyrics, error = %e, "ensureLyricsForIndexing failed");
            }
        });
    }

    pub async fn get_stats(&self) -> AppResult<IndexingStats> {
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*)::int8 FROM tracks")
            .fetch_one(&self.pg)
            .await?;
        let indexed: (i64,) =
            sqlx::query_as("SELECT COUNT(*)::int8 FROM tracks WHERE index_state = 'indexed'")
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
                    let raw_id = data.get("sc_track_id").and_then(|v| v.as_str()).unwrap_or("");
                    let storage_url = data
                        .get("storage_url")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_default();
                    let quality = data
                        .get("quality")
                        .and_then(|v| v.as_str())
                        .unwrap_or("sq");
                    let Some(sc_track_id) = normalize_sc_track_id(raw_id) else {
                        return Ok(());
                    };
                    if storage_url.is_empty() {
                        return Ok(());
                    }

                    let existing = svc.tracks.find_by_sc_track_id(&sc_track_id).await?;
                    let Some(row) = existing else {
                        // Orphan upload — нет родительской tracks-строки.
                        // Это либо backfill-расхождение, либо storage сам по
                        // себе уехал. Не создаём фантомных треков; storage
                        // событие игнорируем.
                        debug!(track = %sc_track_id, "storage uploaded for unknown track — skipping");
                        return Ok(());
                    };

                    svc.tracks.mark_storage_done(&sc_track_id, quality).await?;

                    // Публикуем INDEX_AUDIO только если индексация ещё не
                    // завершена. Re-upload индексированного трека не нужен.
                    if row.index_state != "indexed" {
                        svc.nats
                            .publish(
                                subjects::INDEX_AUDIO,
                                &json!({ "sc_track_id": sc_track_id, "s3_url": storage_url }),
                            )
                            .await?;
                        info!(track = %sc_track_id, "[storage→index] published to NATS");
                    }

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
                    svc.tracks.mark_indexed(sc_track_id).await?;
                    debug!(track = %sc_track_id, "indexed_at set");
                    if let Some(fp) = data.get("fingerprint").and_then(|v| v.as_str()) {
                        if !fp.is_empty() {
                            let canonical = svc.tracks.apply_fingerprint(sc_track_id, fp).await?;
                            if let Some(c) = canonical {
                                debug!(track = %sc_track_id, canonical = %c, "fingerprint canonicalized");
                            }
                        }
                    }
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

    /// Реап «зависших» треков. Два сценария:
    /// * `storage_state='pending'` дольше REAP_AGE — transcode-trigger не дошёл
    ///   (streaming был занят / упал HTTP) или storage не ответил. Триггерим
    ///   повторно — TranscodeTriggerService сам дедупит inflight и сначала
    ///   делает S3-probe: если файл уже в S3 (бэк падал между upload'ом и
    ///   `mark_storage_done`), синтетический `STORAGE_TRACK_UPLOADED` доводит
    ///   цепочку без повторного SC→streaming→S3 roundtrip'а.
    /// * `storage_state='ok'` + `index_state='pending'` — файл уже в S3, но
    ///   qdrant не доехал. Trigger пройдёт по тому же S3-probe path'у и
    ///   опубликует `STORAGE_TRACK_UPLOADED` синтетически → `INDEX_AUDIO`
    ///   уйдёт заново, streaming не дёргаем.
    async fn reap(self: &Arc<Self>) -> AppResult<()> {
        let cutoff = chrono::Utc::now() - chrono::Duration::from_std(REAP_AGE).unwrap_or_default();
        let stuck: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM tracks \
             WHERE created_at < $1 \
               AND ( \
                   storage_state = 'pending' \
                   OR (index_state = 'pending' \
                       AND storage_state = 'ok' \
                       AND s3_verified_at IS NOT NULL) \
               ) \
             ORDER BY index_priority, created_at \
             LIMIT $2",
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
