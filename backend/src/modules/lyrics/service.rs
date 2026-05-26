use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use mini_moka::sync::Cache;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::bus::nats::NatsService;
use crate::bus::subjects::{self, streams};
use crate::error::AppResult;
use crate::modules::lyrics::genius::GeniusService;
use crate::modules::lyrics::lrclib::LrclibService;
use crate::modules::lyrics::musixmatch::MusixmatchService;
use crate::modules::lyrics::netease::NeteaseService;
use crate::modules::lyrics::util::{
    canon_meta, detect_language_heuristic, heuristic_queries, pick_lyrics_text,
    strip_lrc_timestamps,
};
use crate::modules::lyrics::worker_client::{RankCandidate, WorkerClient};
use crate::modules::transcode::TranscodeTriggerService;

const MIN_RANK_SCORE: f32 = 6.0;
const MAX_CANDIDATES: usize = 8;
const SNIPPET_LEN: usize = 220;
const MIN_META_OVERLAP: f32 = 0.25;
const MAX_DURATION_DIFF: f32 = 0.25;

const REAP_INTERVAL: Duration = Duration::from_secs(10 * 60);
const REAP_MIN_AGE: Duration = Duration::from_secs(10 * 60);
const REAP_LIMIT_ALIGN: i64 = 30;
const REAP_LIMIT_FULL: i64 = 20;

const INFLIGHT_CAPACITY: u64 = 4096;
const INFLIGHT_TTL: Duration = Duration::from_secs(5 * 60);

const STOPWORDS: &[&str] = &[
    "feat",
    "ft",
    "featuring",
    "prod",
    "remix",
    "edit",
    "version",
    "mix",
    "cover",
    "live",
    "acoustic",
    "instrumental",
    "original",
    "official",
    "audio",
    "video",
    "lyrics",
    "lyric",
    "sped",
    "slowed",
    "nightcore",
    "reverb",
    "extended",
    "radio",
    "clean",
    "explicit",
    "hd",
    "hq",
    "mv",
];

#[derive(Debug, Clone, Serialize)]
pub struct LyricsResponse {
    #[serde(rename = "scTrackId")]
    pub sc_track_id: Option<String>,
    #[serde(rename = "syncedLrc")]
    pub synced_lrc: Option<String>,
    #[serde(rename = "plainText")]
    pub plain_text: Option<String>,
    pub source: String,
    pub language: Option<String>,
    #[serde(rename = "languageConfidence")]
    pub language_confidence: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct LyricsHints {
    pub title: String,
    pub artist: String,
    pub duration_sec: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct LyricsCacheRow {
    pub sc_track_id: String,
    pub synced_lrc: Option<String>,
    pub plain_text: Option<String>,
    pub source: String,
    pub language: Option<String>,
    pub language_confidence: Option<f32>,
    pub embedded_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
struct Candidate {
    source: String,
    synced_lrc: Option<String>,
    plain_text: Option<String>,
    artist_guess: Option<String>,
    title_guess: Option<String>,
    duration_sec: Option<i64>,
}

pub struct LyricsService {
    pg: PgPool,
    nats: Arc<NatsService>,
    lrclib: Arc<LrclibService>,
    mxm: Arc<MusixmatchService>,
    genius: Arc<GeniusService>,
    netease: Arc<NeteaseService>,
    worker: Arc<WorkerClient>,
    trigger: Arc<TranscodeTriggerService>,
    inflight: Cache<String, Arc<Mutex<Option<LyricsResponse>>>>,
    whisper_inflight: Cache<String, Arc<Mutex<()>>>,
    indexing_sem: Arc<Semaphore>,
}

impl LyricsService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pg: PgPool,
        nats: Arc<NatsService>,
        lrclib: Arc<LrclibService>,
        mxm: Arc<MusixmatchService>,
        genius: Arc<GeniusService>,
        netease: Arc<NeteaseService>,
        worker: Arc<WorkerClient>,
        trigger: Arc<TranscodeTriggerService>,
        indexing_concurrency: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            nats,
            lrclib,
            mxm,
            genius,
            netease,
            worker,
            trigger,
            inflight: Cache::builder()
                .max_capacity(INFLIGHT_CAPACITY)
                .time_to_idle(INFLIGHT_TTL)
                .build(),
            whisper_inflight: Cache::builder()
                .max_capacity(INFLIGHT_CAPACITY)
                .time_to_idle(INFLIGHT_TTL)
                .build(),
            indexing_sem: Arc::new(Semaphore::new(indexing_concurrency.max(1))),
        })
    }

    pub fn spawn_consumers(self: &Arc<Self>) {
        let svc = self.clone();
        self.nats.consume(
            streams::DONE.name,
            "backend-done-embed-lyrics",
            Some(subjects::DONE_EMBED_LYRICS),
            move |data| {
                let svc = svc.clone();
                async move {
                    let sc_track_id = data
                        .get("sc_track_id")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let skipped = data
                        .get("skipped")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let Some(id) = sc_track_id else { return Ok(()) };
                    if skipped {
                        return Ok(());
                    }
                    sqlx::query(
                        "UPDATE lyrics_cache SET embedded_at = now() \
                         WHERE sc_track_id = $1 AND embedded_at IS NULL",
                    )
                    .bind(&id)
                    .execute(&svc.pg)
                    .await?;
                    Ok(())
                }
            },
        );
    }

    pub fn spawn_reap_loops(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        let token = shutdown.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(REAP_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = svc.reap_whisper().await {
                            debug!(error = %e, "reap_whisper failed");
                        }
                        if let Err(e) = svc.reap_embeds().await {
                            debug!(error = %e, "reap_embeds failed");
                        }
                    }
                }
            }
        });
    }

    pub async fn ensure_lyrics(
        self: &Arc<Self>,
        sc_track_id_raw: &str,
    ) -> AppResult<LyricsResponse> {
        let sc_track_id = normalize(sc_track_id_raw);

        let cached: Option<LyricsCacheRow> =
            sqlx::query_as("SELECT sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at FROM lyrics_cache WHERE sc_track_id = $1")
                .bind(&sc_track_id)
                .fetch_optional(&self.pg)
                .await?;
        if let Some(row) = cached {
            if row.embedded_at.is_none() {
                if let Some(text) =
                    pick_lyrics_text(row.plain_text.as_deref(), row.synced_lrc.as_deref())
                {
                    if text.len() > 30 {
                        let svc = self.clone();
                        let row_clone = row.clone();
                        tokio::spawn(async move {
                            if let Err(e) = svc.after_found(&row_clone, &text).await {
                                warn!(track = %row_clone.sc_track_id, error = %e, "re-embed retry failed");
                            }
                        });
                    }
                }
            }
            return Ok(to_response(&row));
        }

        let lock = match self.inflight.get(&sc_track_id) {
            Some(l) => l,
            None => {
                let l = Arc::new(Mutex::new(None));
                self.inflight.insert(sc_track_id.clone(), l.clone());
                l
            }
        };
        let mut guard = lock.lock().await;
        if let Some(resp) = guard.clone() {
            return Ok(resp);
        }

        let hints = self.load_hints_from_db(&sc_track_id).await?;
        let result = self
            .run_pipeline(Some(sc_track_id.as_str()), &hints, true)
            .await?;
        *guard = Some(result.clone());
        let id = sc_track_id.clone();
        let cache = self.inflight.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            cache.invalidate(&id);
        });
        Ok(result)
    }

    pub async fn search_lyrics(self: &Arc<Self>, hints: &LyricsHints) -> AppResult<LyricsResponse> {
        if hints.title.is_empty() || hints.artist.is_empty() {
            return Ok(empty_response(None));
        }
        self.run_pipeline(None, hints, false).await
    }

    pub async fn ensure_lyrics_for_indexing(
        self: &Arc<Self>,
        sc_track_id_raw: &str,
    ) -> AppResult<()> {
        let sc_track_id = normalize(sc_track_id_raw);
        if sc_track_id.is_empty() {
            return Ok(());
        }
        let permit = match self.indexing_sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        let svc = self.clone();
        let id = sc_track_id.clone();
        if let Err(e) = svc.ensure_lyrics(&id).await {
            debug!(track = %id, error = %e, "ensureLyricsForIndexing failed");
        }
        drop(permit);
        Ok(())
    }

    async fn run_pipeline(
        self: &Arc<Self>,
        sc_track_id: Option<&str>,
        hints: &LyricsHints,
        allow_save: bool,
    ) -> AppResult<LyricsResponse> {
        let artist = hints.artist.trim();
        let title = hints.title.trim();
        let duration_sec = hints.duration_sec.unwrap_or(0);
        let log_id = sc_track_id
            .map(String::from)
            .unwrap_or_else(|| format!("{artist} - {title}"));

        if title.is_empty() {
            warn!(log_id = %log_id, "lyrics: empty title, skip");
            return Ok(empty_response(sc_track_id));
        }

        let picked = self
            .find_lyrics(&log_id, artist, title, duration_sec)
            .await?;
        if picked.plain_text.is_none() && picked.synced_lrc.is_none() {
            info!(log_id = %log_id, "no lyrics found — not caching");
            return Ok(empty_response(sc_track_id));
        }

        if !allow_save || sc_track_id.is_none() {
            return Ok(LyricsResponse {
                sc_track_id: sc_track_id.map(String::from),
                synced_lrc: picked.synced_lrc.clone(),
                plain_text: picked.plain_text.clone(),
                source: picked.source.clone(),
                language: None,
                language_confidence: None,
            });
        }
        let sc_track_id = sc_track_id.unwrap();

        let row: LyricsCacheRow = sqlx::query_as(
            "INSERT INTO lyrics_cache (sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at) \
             VALUES ($1, $2, $3, $4, NULL, NULL, NULL) RETURNING sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at",
        )
        .bind(sc_track_id)
        .bind(&picked.synced_lrc)
        .bind(&picked.plain_text)
        .bind(&picked.source)
        .fetch_one(&self.pg)
        .await?;

        if let Some(text) =
            pick_lyrics_text(picked.plain_text.as_deref(), picked.synced_lrc.as_deref())
        {
            if text.len() > 30 {
                let svc = self.clone();
                let row_clone = row.clone();
                tokio::spawn(async move {
                    if let Err(e) = svc.after_found(&row_clone, &text).await {
                        warn!(track = %row_clone.sc_track_id, error = %e, "after-found failed");
                    }
                });
            }
        }

        Ok(to_response(&row))
    }

    async fn load_hints_from_db(&self, sc_track_id: &str) -> AppResult<LyricsHints> {
        // Источники artist для лирики, в порядке предпочтения:
        // 1. metadata_artist из SC payload (наиболее каноничный для лейбловых
        //    upload'ов; раньше брался из publisher_metadata.artist);
        // 2. uploader_username (= user.username).
        let row: Option<(String, i32, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT title, duration_ms, metadata_artist, uploader_username \
             FROM tracks WHERE sc_track_id = $1",
        )
        .bind(sc_track_id)
        .fetch_optional(&self.pg)
        .await?;
        let (title, dur_ms, artist) = match row {
            Some((t, d, meta, uploader)) => {
                let artist = meta.or(uploader).unwrap_or_default();
                (t, d as i64, artist)
            }
            None => (String::new(), 0i64, String::new()),
        };
        Ok(LyricsHints {
            title,
            artist,
            duration_sec: if dur_ms > 0 {
                Some((dur_ms as f64 / 1000.0).round() as i64)
            } else {
                None
            },
        })
    }

    async fn find_lyrics(
        self: &Arc<Self>,
        log_id: &str,
        artist: &str,
        title: &str,
        duration_sec: i64,
    ) -> AppResult<Candidate> {
        info!(log_id, artist, title, duration_sec, "findLyrics");

        let heuristics = heuristic_queries(artist, title);
        info!(log_id, queries = ?heuristics, "[stage1] queries");
        if let Some(pick) = self
            .search_and_pick(&heuristics, artist, title, duration_sec, log_id, "[stage1]")
            .await?
        {
            return Ok(pick);
        }

        let llm_queries = self
            .worker
            .generate_search_queries(artist, title)
            .await
            .unwrap_or_default();
        info!(log_id, queries = ?llm_queries, "[stage2] queries");
        let heur_lower: HashSet<String> = heuristics.iter().map(|q| q.to_lowercase()).collect();
        let new_queries: Vec<String> = llm_queries
            .iter()
            .filter(|q| !heur_lower.contains(&q.trim().to_lowercase()))
            .cloned()
            .collect();
        if !new_queries.is_empty() {
            if let Some(pick) = self
                .search_and_pick(
                    &llm_queries,
                    artist,
                    title,
                    duration_sec,
                    log_id,
                    "[stage2]",
                )
                .await?
            {
                return Ok(pick);
            }
        } else {
            info!(log_id, "[stage2] LLM added nothing new, skipping fanout");
        }

        Ok(Candidate {
            source: "none".into(),
            synced_lrc: None,
            plain_text: None,
            artist_guess: None,
            title_guess: None,
            duration_sec: None,
        })
    }

    async fn search_and_pick(
        self: &Arc<Self>,
        queries: &[String],
        artist: &str,
        title: &str,
        duration_sec: i64,
        sc_track_id: &str,
        stage: &str,
    ) -> AppResult<Option<Candidate>> {
        let raw = self.fanout_search(queries).await;
        info!(stage, sc_track_id, count = raw.len(), "raw candidates");
        let candidates = self.filter_by_metadata(raw, artist, title, duration_sec, sc_track_id);
        if candidates.is_empty() {
            info!(stage, sc_track_id, "no candidates survived metadata filter");
            return Ok(None);
        }
        if let Some(exact) =
            self.pick_exact_match(&candidates, queries, artist, title, sc_track_id, stage)
        {
            return Ok(Some(exact));
        }
        let rank_cands: Vec<RankCandidate> = candidates
            .iter()
            .enumerate()
            .map(|(idx, c)| RankCandidate {
                idx,
                source: c.source.clone(),
                snippet: build_snippet(c),
            })
            .collect();
        let ranked = self.worker.rank_lyrics(artist, title, &rank_cands).await?;
        info!(stage, sc_track_id, ranked = ?ranked, "rank result");
        if let Some(r) = ranked {
            if r.score >= MIN_RANK_SCORE {
                if let Some(pick) = candidates.get(r.best_idx) {
                    info!(
                        stage,
                        sc_track_id,
                        source = %pick.source,
                        score = r.score,
                        "picked"
                    );
                    return Ok(Some(pick.clone()));
                }
            }
        }
        Ok(None)
    }

    fn pick_exact_match(
        &self,
        candidates: &[Candidate],
        queries: &[String],
        artist: &str,
        title: &str,
        sc_track_id: &str,
        stage: &str,
    ) -> Option<Candidate> {
        let a = canon_meta(artist);
        let t = canon_meta(title);
        let mut query_set: HashSet<String> = HashSet::new();
        for q in queries {
            let c = canon_meta(q);
            if !c.is_empty() {
                query_set.insert(c);
            }
        }
        for c in candidates {
            let ca = canon_meta(c.artist_guess.as_deref().unwrap_or(""));
            let ct = canon_meta(c.title_guess.as_deref().unwrap_or(""));
            if ca.is_empty() || ct.is_empty() {
                continue;
            }
            if !a.is_empty() && !t.is_empty() && ca == a && ct == t {
                info!(stage, sc_track_id, "exact match (direct)");
                return Some(c.clone());
            }
            let fwd = format!("{ca} {ct}");
            let rev = format!("{ct} {ca}");
            if query_set.contains(&fwd) || query_set.contains(&rev) {
                info!(stage, sc_track_id, "exact match (via query)");
                return Some(c.clone());
            }
        }
        None
    }

    fn filter_by_metadata(
        &self,
        candidates: Vec<Candidate>,
        artist: &str,
        title: &str,
        duration_sec: i64,
        sc_track_id: &str,
    ) -> Vec<Candidate> {
        let source = format!("{artist} {title}").trim().to_string();
        let mut out: Vec<Candidate> = Vec::new();
        let total = candidates.len();
        for c in candidates {
            let cand_meta = format!(
                "{} {}",
                c.artist_guess.as_deref().unwrap_or(""),
                c.title_guess.as_deref().unwrap_or("")
            )
            .trim()
            .to_string();
            if !cand_meta.is_empty() {
                let overlap = meta_overlap(&source, &cand_meta);
                if overlap < MIN_META_OVERLAP {
                    debug!(
                        track = %sc_track_id,
                        source = %c.source,
                        overlap,
                        "drop: low meta overlap"
                    );
                    continue;
                }
            }
            if duration_sec > 0 {
                if let Some(d) = c.duration_sec {
                    let max = duration_sec.max(d) as f32;
                    if max > 0.0 {
                        let diff = (duration_sec as f32 - d as f32).abs() / max;
                        if diff > MAX_DURATION_DIFF {
                            debug!(
                                track = %sc_track_id,
                                source = %c.source,
                                diff,
                                "drop: duration mismatch"
                            );
                            continue;
                        }
                    }
                }
            }
            out.push(c);
        }
        info!(
            track = %sc_track_id,
            kept = out.len(),
            total,
            "metadata filter"
        );
        out
    }

    async fn fanout_search(&self, queries: &[String]) -> Vec<Candidate> {
        let mut seen = HashSet::new();
        let mut unique: Vec<String> = Vec::new();
        for q in queries
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            if seen.insert(q.clone()) {
                unique.push(q);
            }
            if unique.len() >= 4 {
                break;
            }
        }

        let mut tasks: Vec<tokio::task::JoinHandle<Vec<Candidate>>> = Vec::new();
        for q in &unique {
            let q_clone = q.clone();
            let lrc = self.lrclib.clone();
            tasks.push(tokio::spawn(async move {
                lrc.search_by_query(&q_clone, 10)
                    .await
                    .into_iter()
                    .map(|r| {
                        let plain = r
                            .plain_text
                            .clone()
                            .or_else(|| r.synced_lrc.as_deref().map(strip_lrc_timestamps));
                        Candidate {
                            source: "lrclib".into(),
                            synced_lrc: r.synced_lrc,
                            plain_text: plain,
                            artist_guess: r.artist_guess,
                            title_guess: r.title_guess,
                            duration_sec: r.duration_sec,
                        }
                    })
                    .collect()
            }));

            let q_clone = q.clone();
            let mxm = self.mxm.clone();
            tasks.push(tokio::spawn(async move {
                mxm.search_by_query(&q_clone, 10)
                    .await
                    .into_iter()
                    .map(|r| {
                        let plain = r
                            .plain_text
                            .clone()
                            .or_else(|| r.synced_lrc.as_deref().map(strip_lrc_timestamps));
                        Candidate {
                            source: "musixmatch".into(),
                            synced_lrc: r.synced_lrc,
                            plain_text: plain,
                            artist_guess: r.artist_guess,
                            title_guess: r.title_guess,
                            duration_sec: r.duration_sec,
                        }
                    })
                    .collect()
            }));

            let q_clone = q.clone();
            let gen = self.genius.clone();
            tasks.push(tokio::spawn(async move {
                gen.search_by_query(&q_clone, 10)
                    .await
                    .into_iter()
                    .map(|r| Candidate {
                        source: "genius".into(),
                        synced_lrc: None,
                        plain_text: Some(r.plain_text),
                        artist_guess: r.artist_guess,
                        title_guess: r.title_guess,
                        duration_sec: None,
                    })
                    .collect()
            }));

            let q_clone = q.clone();
            let net = self.netease.clone();
            tasks.push(tokio::spawn(async move {
                net.search_by_query(&q_clone, 5)
                    .await
                    .into_iter()
                    .map(|r| {
                        let plain = r
                            .plain_text
                            .clone()
                            .or_else(|| r.synced_lrc.as_deref().map(strip_lrc_timestamps));
                        Candidate {
                            source: "netease".into(),
                            synced_lrc: r.synced_lrc,
                            plain_text: plain,
                            artist_guess: r.artist_guess,
                            title_guess: r.title_guess,
                            duration_sec: r.duration_sec,
                        }
                    })
                    .collect()
            }));
        }

        let mut all: Vec<Candidate> = Vec::new();
        for t in tasks {
            if let Ok(items) = t.await {
                all.extend(items);
            }
        }
        let deduped = dedupe(all);
        deduped.into_iter().take(MAX_CANDIDATES).collect()
    }

    pub async fn handle_uploaded(self: &Arc<Self>, sc_track_id_raw: &str, storage_url: &str) {
        let sc_track_id = normalize(sc_track_id_raw);
        if sc_track_id.is_empty() || storage_url.is_empty() {
            return;
        }

        let lock = match self.whisper_inflight.get(&sc_track_id) {
            Some(l) => l,
            None => {
                let l = Arc::new(Mutex::new(()));
                self.whisper_inflight.insert(sc_track_id.clone(), l.clone());
                l
            }
        };
        let _g = lock.lock().await;

        let entity: Option<LyricsCacheRow> = match sqlx::query_as(
            "SELECT sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at FROM lyrics_cache WHERE sc_track_id = $1",
        )
        .bind(&sc_track_id)
        .fetch_optional(&self.pg)
        .await
        {
            Ok(v) => v,
            Err(e) => {
                warn!(track = %sc_track_id, error = %e, "handle_uploaded: db read failed");
                return;
            }
        };

        if let Some(row) = entity.as_ref() {
            if row.synced_lrc.is_some() {
                return;
            }
            if row.plain_text.is_some() {
                if let Err(e) = self.align_with_whisper(row, storage_url).await {
                    warn!(track = %sc_track_id, error = %e, "align_with_whisper failed");
                }
                return;
            }
        }

        if let Err(e) = self.full_transcribe(&sc_track_id, storage_url).await {
            warn!(track = %sc_track_id, error = %e, "full_transcribe failed");
        }
    }

    async fn align_with_whisper(
        self: &Arc<Self>,
        entity: &LyricsCacheRow,
        storage_url: &str,
    ) -> AppResult<()> {
        let plain = match entity.plain_text.as_deref() {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(()),
        };
        if entity.synced_lrc.is_some() {
            return Ok(());
        }
        let lang = entity.language.clone();
        let initial = plain.chars().take(2000).collect::<String>();
        let result = match self
            .worker
            .transcribe_audio(storage_url, lang.as_deref(), Some(&initial))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(track = %entity.sc_track_id, error = %e, "align: transcribe failed");
                return Ok(());
            }
        };
        let Some(res) = result else { return Ok(()) };
        let Some(synced) = res.synced_lrc else {
            return Ok(());
        };
        sqlx::query("UPDATE lyrics_cache SET synced_lrc = $2 WHERE sc_track_id = $1")
            .bind(&entity.sc_track_id)
            .bind(&synced)
            .execute(&self.pg)
            .await?;
        info!(track = %entity.sc_track_id, "aligned sync LRC");
        Ok(())
    }

    async fn full_transcribe(
        self: &Arc<Self>,
        sc_track_id: &str,
        storage_url: &str,
    ) -> AppResult<()> {
        let result = match self.worker.transcribe_audio(storage_url, None, None).await {
            Ok(r) => r,
            Err(e) => {
                warn!(track = %sc_track_id, error = %e, "self-gen transcribe failed");
                return Ok(());
            }
        };
        let Some(res) = result else { return Ok(()) };
        if res.synced_lrc.is_none() && res.plain_text.is_none() {
            info!(track = %sc_track_id, "self-gen: whisper returned empty");
            return Ok(());
        }
        let row: LyricsCacheRow = sqlx::query_as(
            "INSERT INTO lyrics_cache (sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at) \
             VALUES ($1, $2, $3, 'self_gen', NULL, NULL, NULL) RETURNING sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at",
        )
        .bind(sc_track_id)
        .bind(&res.synced_lrc)
        .bind(&res.plain_text)
        .fetch_one(&self.pg)
        .await?;
        info!(track = %sc_track_id, lang = %res.language, "self-generated LRC");
        if let Some(text) = pick_lyrics_text(res.plain_text.as_deref(), res.synced_lrc.as_deref()) {
            if text.len() > 30 {
                let svc = self.clone();
                let row_clone = row.clone();
                tokio::spawn(async move {
                    if let Err(e) = svc.after_found(&row_clone, &text).await {
                        warn!(track = %row_clone.sc_track_id, error = %e, "after-found failed");
                    }
                });
            }
        }
        Ok(())
    }

    async fn after_found(&self, entity: &LyricsCacheRow, text: &str) -> AppResult<()> {
        let lang_input = text.chars().take(2000).collect::<String>();
        let mut lang = match self.worker.detect_language(&lang_input).await {
            Ok(v) => v,
            Err(e) => {
                debug!(track = %entity.sc_track_id, error = %e, "detectLanguage worker error");
                None
            }
        };
        if lang.is_none() {
            lang = detect_language_heuristic(text).map(|h| {
                crate::modules::lyrics::worker_client::LangResult {
                    language: h.language,
                    confidence: h.confidence,
                }
            });
            if let Some(l) = &lang {
                info!(track = %entity.sc_track_id, language = %l.language, confidence = l.confidence, "detectLanguage heuristic");
            } else {
                warn!(track = %entity.sc_track_id, "detectLanguage: both worker and heuristic returned null");
            }
        } else if let Some(l) = &lang {
            info!(track = %entity.sc_track_id, language = %l.language, confidence = l.confidence, "detectLanguage worker");
        }
        let final_lang = lang.as_ref().map(|l| l.language.clone());
        if let Some(l) = &lang {
            sqlx::query(
                "UPDATE lyrics_cache SET language = $2, language_confidence = $3 WHERE sc_track_id = $1",
            )
            .bind(&entity.sc_track_id)
            .bind(&l.language)
            .bind(l.confidence)
            .execute(&self.pg)
            .await?;
            sqlx::query(
                "UPDATE tracks SET language = $2, language_confidence = $3 WHERE sc_track_id = $1",
            )
            .bind(&entity.sc_track_id)
            .bind(&l.language)
            .bind(l.confidence)
            .execute(&self.pg)
            .await?;
        }

        let body = serde_json::json!({
            "sc_track_id": entity.sc_track_id,
            "text": text.chars().take(4000).collect::<String>(),
            "language": final_lang,
        });
        if let Err(e) = self.nats.publish(subjects::EMBED_LYRICS, &body).await {
            warn!(track = %entity.sc_track_id, error = %e, "embed publish failed");
        }
        Ok(())
    }

    async fn reap_whisper(self: &Arc<Self>) -> AppResult<()> {
        let cutoff =
            chrono::Utc::now().naive_utc() - chrono::Duration::from_std(REAP_MIN_AGE).unwrap();

        let need_align: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM lyrics_cache \
             WHERE plain_text IS NOT NULL AND length(plain_text) > 0 AND synced_lrc IS NULL AND created_at < $1 \
             ORDER BY created_at ASC LIMIT $2",
        )
        .bind(cutoff)
        .bind(REAP_LIMIT_ALIGN)
        .fetch_all(&self.pg)
        .await?;

        let need_full: Vec<(String,)> = sqlx::query_as(
            "SELECT it.sc_track_id FROM tracks it \
             LEFT JOIN lyrics_cache lc ON lc.sc_track_id = it.sc_track_id \
             WHERE it.storage_state IN ('sq','hq') AND lc.sc_track_id IS NULL AND it.created_at < $1 \
             ORDER BY it.created_at ASC LIMIT $2",
        )
        .bind(cutoff)
        .bind(REAP_LIMIT_FULL)
        .fetch_all(&self.pg)
        .await?;

        let total = need_align.len() + need_full.len();
        if total == 0 {
            return Ok(());
        }
        info!(
            align = need_align.len(),
            full = need_full.len(),
            "[lyrics-reap] retrying whisper"
        );
        for (id,) in need_align.into_iter().chain(need_full) {
            self.trigger.trigger(&id);
        }
        Ok(())
    }

    async fn reap_embeds(self: &Arc<Self>) -> AppResult<()> {
        let cutoff =
            chrono::Utc::now().naive_utc() - chrono::Duration::from_std(REAP_MIN_AGE).unwrap();
        let stuck: Vec<LyricsCacheRow> = sqlx::query_as(
            "SELECT sc_track_id, synced_lrc, plain_text, source, language, language_confidence, embedded_at FROM lyrics_cache \
             WHERE embedded_at IS NULL AND created_at < $1 \
             AND length(coalesce(plain_text, synced_lrc, '')) > 30 \
             ORDER BY created_at ASC LIMIT $2",
        )
        .bind(cutoff)
        .bind(REAP_LIMIT_FULL)
        .fetch_all(&self.pg)
        .await?;
        if stuck.is_empty() {
            return Ok(());
        }
        info!(count = stuck.len(), "[lyrics-reap] re-publishing embed");
        for row in stuck {
            let Some(text) = pick_lyrics_text(row.plain_text.as_deref(), row.synced_lrc.as_deref())
            else {
                continue;
            };
            if text.len() <= 30 {
                continue;
            }
            let svc = self.clone();
            let row_clone = row.clone();
            tokio::spawn(async move {
                if let Err(e) = svc.after_found(&row_clone, &text).await {
                    warn!(track = %row_clone.sc_track_id, error = %e, "embed-reap failed");
                }
            });
        }
        Ok(())
    }
}

fn empty_response(sc_track_id: Option<&str>) -> LyricsResponse {
    LyricsResponse {
        sc_track_id: sc_track_id.map(String::from),
        synced_lrc: None,
        plain_text: None,
        source: "none".into(),
        language: None,
        language_confidence: None,
    }
}

fn to_response(row: &LyricsCacheRow) -> LyricsResponse {
    LyricsResponse {
        sc_track_id: Some(row.sc_track_id.clone()),
        synced_lrc: row.synced_lrc.clone(),
        plain_text: row.plain_text.clone(),
        source: row.source.clone(),
        language: row.language.clone(),
        language_confidence: row.language_confidence,
    }
}

fn build_snippet(c: &Candidate) -> String {
    let text = c.plain_text.clone().unwrap_or_else(|| {
        c.synced_lrc
            .as_deref()
            .map(strip_lrc_timestamps)
            .unwrap_or_default()
    });
    let guess = if c.artist_guess.is_some() || c.title_guess.is_some() {
        format!(
            "({} — {}) ",
            c.artist_guess.as_deref().unwrap_or("?"),
            c.title_guess.as_deref().unwrap_or("?")
        )
    } else {
        String::new()
    };
    let combined = format!("{guess}{text}");
    combined.chars().take(SNIPPET_LEN).collect()
}

fn dedupe(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for c in candidates {
        let body_src = c
            .plain_text
            .clone()
            .or_else(|| c.synced_lrc.clone())
            .unwrap_or_default();
        let body: String = body_src.chars().take(80).collect();
        let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            continue;
        }
        let key = collapsed.to_lowercase();
        if seen.insert(key) {
            out.push(c);
        }
    }
    out
}

fn tokenize(s: &str) -> HashSet<String> {
    let lowered = s.to_lowercase();
    let mut buf = String::with_capacity(lowered.len());
    let mut skip_paren = false;
    let mut skip_bracket = false;
    for ch in lowered.chars() {
        match ch {
            '(' => {
                skip_paren = true;
                buf.push(' ');
            }
            ')' => {
                skip_paren = false;
                buf.push(' ');
            }
            '[' => {
                skip_bracket = true;
                buf.push(' ');
            }
            ']' => {
                skip_bracket = false;
                buf.push(' ');
            }
            _ if skip_paren || skip_bracket => buf.push(' '),
            _ if ch.is_alphanumeric() || ch.is_whitespace() => buf.push(ch),
            _ => buf.push(' '),
        }
    }
    let mut out = HashSet::new();
    for t in buf.split_whitespace() {
        if t.len() < 2 {
            continue;
        }
        if STOPWORDS.contains(&t) {
            continue;
        }
        out.insert(t.to_string());
    }
    out
}

fn meta_overlap(src: &str, cand: &str) -> f32 {
    let a = tokenize(src);
    let b = tokenize(cand);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let common = a.iter().filter(|t| b.contains(*t)).count();
    let min = a.len().min(b.len()) as f32;
    common as f32 / min
}

fn normalize(raw: &str) -> String {
    let s = raw.trim();
    match s.rfind(':') {
        Some(idx) => s[idx + 1..].to_string(),
        None => s.to_string(),
    }
}
