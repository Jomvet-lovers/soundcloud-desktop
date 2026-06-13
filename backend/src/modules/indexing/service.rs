use std::sync::Arc;
use std::time::Duration;

use mini_moka::sync::Cache;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::OnceCell;
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
use crate::modules::work::Kicker;
use crate::qdrant::{parse_f32_vec, QdrantService};

const REAP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const REAP_AGE: Duration = Duration::from_secs(5 * 60);
const REAP_BATCH: i64 = 50;
/// Сколько storage-реджектов подряд переводят трек в `storage_state='failed'`.
const STORAGE_REJECT_MAX_ATTEMPTS: i32 = 3;
/// Дедуп редоставок rejected-события (NATS at-least-once, ack_wait 120s);
/// реальные повторные реджекты идут реже — не глушатся.
const STORAGE_REJECT_DEDUP_TTL: Duration = Duration::from_secs(240);
/// `failed` ретраим редко: реджект сам обновляет updated_at и отодвигает
/// следующую попытку, так что один трек стоит максимум скачку в сутки.
const FAILED_RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const FAILED_RETRY_BATCH: i64 = 10;

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
    qdrant: Arc<QdrantService>,
    lyrics: Arc<LyricsService>,
    trigger: Arc<TranscodeTriggerService>,
    tracks: TrackRepository,
    max_track_duration_ms: i32,
    enrich_kick: OnceCell<Kicker>,
    rejected_dedup: Cache<String, ()>,
}

impl IndexingService {
    pub fn new(
        pg: PgPool,
        nats: Arc<NatsService>,
        qdrant: Arc<QdrantService>,
        lyrics: Arc<LyricsService>,
        trigger: Arc<TranscodeTriggerService>,
        max_track_duration_ms: i32,
    ) -> Arc<Self> {
        let tracks = TrackRepository::new(pg.clone());
        Arc::new(Self {
            pg,
            nats,
            qdrant,
            lyrics,
            trigger,
            tracks,
            max_track_duration_ms,
            enrich_kick: OnceCell::new(),
            rejected_dedup: Cache::builder()
                .max_capacity(16_384)
                .time_to_live(STORAGE_REJECT_DEDUP_TTL)
                .build(),
        })
    }

    /// Wire the enrich pool's kick sender (set once, after enrich.spawn()).
    pub fn install_enrich_kicker(&self, kicker: Kicker) {
        let _ = self.enrich_kick.set(kicker);
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        self.subscribe_done();
        self.subscribe_storage_uploaded();
        self.subscribe_storage_rejected();
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
        if self.max_track_duration_ms > 0 && fields.duration_ms > self.max_track_duration_ms {
            self.tracks.mark_too_long(&fields.sc_track_id).await?;
            return Ok(());
        }
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
        // Enrich: in-process kick (no broker). The row is already
        // enrich_state='pending' from upsert_from_sc, so even a dropped kick is
        // picked up by the pool's next priority-ordered claim.
        if let Some(kicker) = self.enrich_kick.get() {
            kicker.kick(sc_track_id.to_string());
        }
        let lyrics = self.lyrics.clone();
        let id_for_lyrics = sc_track_id.to_string();
        tokio::spawn(async move {
            if let Err(e) = lyrics.ensure_lyrics_for_indexing(&id_for_lyrics).await {
                debug!(track = %id_for_lyrics, error = %e, "ensureLyricsForIndexing failed");
            }
        });
    }

    pub async fn get_stats(&self) -> AppResult<IndexingStats> {
        let total = sqlx::query_file_scalar!("queries/indexing/service/count_tracks.sql")
            .fetch_one(&self.pg)
            .await?;
        let indexed = sqlx::query_file_scalar!("queries/indexing/service/count_indexed.sql")
            .fetch_one(&self.pg)
            .await?;
        Ok(IndexingStats {
            indexed,
            pending: total - indexed,
        })
    }

    fn subscribe_storage_uploaded(self: &Arc<Self>) {
        let svc = self.clone();
        self.nats.consume(
            streams::STORAGE_EVENTS.name,
            "backend-storage-uploaded",
            Some(subjects::STORAGE_TRACK_UPLOADED),
            16,
            move |data| {
                let svc = svc.clone();
                async move {
                    let raw_id = data.get("sc_track_id").and_then(|v| v.as_str()).unwrap_or("");
                    let storage_url = data
                        .get("storage_url")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .unwrap_or_default();
                    // `None` → синтетический S3-hit event без quality: не
                    // даунгрейдим уже записанное `storage_quality` (см.
                    // `mark_storage_done`). Реальный storage-event несёт quality.
                    let quality = data.get("quality").and_then(|v| v.as_str());
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
                    // завершена и трек не terminal too_long (>7min —
                    // не скачиваем/не индексируем, воркеру делать нечего).
                    // Проверяем и duration_ms на случай race с duration_resolver.
                    let is_too_long = row.storage_state == "too_long"
                        || row.index_state == "too_long"
                        || (svc.max_track_duration_ms > 0
                            && row.duration_ms > svc.max_track_duration_ms);
                    if is_too_long {
                        // race: duration resolver мог ещё не дойти — помечаем
                        svc.tracks.mark_too_long(&sc_track_id).await?;
                        debug!(track = %sc_track_id, "too_long — skipped INDEX_AUDIO");
                    } else if row.index_state != "indexed" {
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

    /// `storage.track_rejected` — storage забраковал скачанный файл (duration
    /// mismatch / too short / too long). Копим страйки до 'failed', чтобы не
    /// жечь SC-квоту перекачкой заведомо бракуемого файла каждые 5 минут.
    fn subscribe_storage_rejected(self: &Arc<Self>) {
        let svc = self.clone();
        self.nats.consume(
            streams::STORAGE_EVENTS.name,
            "backend-storage-rejected",
            Some(subjects::STORAGE_TRACK_REJECTED),
            16,
            move |data| {
                let svc = svc.clone();
                async move {
                    let raw_id = data
                        .get("sc_track_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let Some(sc_track_id) = normalize_sc_track_id(raw_id) else {
                        return Ok(());
                    };
                    let reason = data
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let actual_secs = data
                        .get("actual_secs")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let expected_ms = data.get("expected_duration_ms").and_then(|v| v.as_i64());
                    if svc.rejected_dedup.get(&sc_track_id).is_some() {
                        return Ok(());
                    }
                    svc.tracks
                        .mark_storage_rejected(&sc_track_id, STORAGE_REJECT_MAX_ATTEMPTS)
                        .await?;
                    // В дедуп только после успешного UPDATE — ошибка должна
                    // редоставиться и досчитаться.
                    svc.rejected_dedup.insert(sc_track_id.clone(), ());
                    svc.trigger.invalidate_inflight(&sc_track_id);
                    warn!(
                        track = %sc_track_id,
                        reason,
                        actual_secs,
                        expected_ms = ?expected_ms,
                        "storage rejected upload"
                    );
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
            16,
            move |data| {
                let svc = svc.clone();
                async move {
                    let Some(sc_track_id) = data.get("sc_track_id").and_then(|v| v.as_str()) else {
                        return Ok(());
                    };
                    // Воркер шлёт вектора в payload — пишем их в Qdrant ДО mark_indexed.
                    // Upsert упал → Err → NAK → передоставка (вектора не потеряем).
                    if let (Some(mert), Some(clap)) = (
                        parse_f32_vec(data.get("mert")),
                        parse_f32_vec(data.get("clap")),
                    ) {
                        if let Ok(id) = sc_track_id.parse::<u64>() {
                            let language = data.get("language").and_then(|v| v.as_str());
                            svc.qdrant.upsert_audio(id, mert, clap, language).await?;
                        }
                    }
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

    /// Реап «зависших» треков. Три сценария:
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
    /// * `storage_state='failed'` тише суток — редкий ретрай: SC мог сменить
    ///   отдачу (Go+ открылся, файл заменён). Повторный реджект снова отодвинет
    ///   updated_at, успех снимет failed через mark_storage_done.
    async fn reap(self: &Arc<Self>) -> AppResult<()> {
        let cutoff = chrono::Utc::now() - chrono::Duration::from_std(REAP_AGE).unwrap_or_default();
        let stuck = sqlx::query_file_scalar!(
            "queries/indexing/service/reap_stuck.sql",
            cutoff,
            REAP_BATCH
        )
        .fetch_all(&self.pg)
        .await?;
        let retry_cutoff =
            chrono::Utc::now() - chrono::Duration::from_std(FAILED_RETRY_AFTER).unwrap_or_default();
        let failed = sqlx::query_file_scalar!(
            "queries/indexing/service/reap_failed_retry.sql",
            retry_cutoff,
            FAILED_RETRY_BATCH
        )
        .fetch_all(&self.pg)
        .await?;
        if stuck.is_empty() && failed.is_empty() {
            return Ok(());
        }
        info!(
            stuck = stuck.len(),
            failed_retries = failed.len(),
            "indexing reap: retriggering tracks"
        );
        for id in stuck.into_iter().chain(failed) {
            self.trigger.trigger(&id);
        }
        Ok(())
    }
}
