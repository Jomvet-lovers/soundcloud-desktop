use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::config::EnrichCrawlCfg;
use crate::db::advisory_locks;
use crate::error::AppResult;
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::enrich::ai_matcher::{AiMatcherClient, MatchCandidate, MatchTarget};
use crate::modules::enrich::matcher::{evaluate_sc_candidate, sc_track_id_from_urn};
use crate::modules::enrich::sc_account_scan::{ScAccountScanner, WantedRow};
use crate::modules::indexing::IndexingService;
use crate::modules::tracks::TrackPriority;
use crate::sc::ScClient;

const BATCH_SIZE: i64 = 30;
const SEARCH_LIMIT: usize = 10;
const STAGE2_CONCURRENCY: usize = 8;
/// Композитный score для безусловной линковки. Что в диапазоне
/// [BORDERLINE_LOW, SEARCH_LINK_THRESHOLD) — отдаётся на AI matcher (если включён).
const SEARCH_LINK_THRESHOLD: f32 = 0.7;
/// Нижняя граница «borderline»-зоны: ниже — сразу отбрасываем как mismatch.
const BORDERLINE_LOW: f32 = 0.45;

pub struct WantedResolverService {
    pg: PgPool,
    sc: ScClient,
    tokens: Arc<TokenProvider>,
    indexing: Arc<IndexingService>,
    scanner: Arc<ScAccountScanner>,
    ai_matcher: Option<Arc<AiMatcherClient>>,
    interval: Duration,
}

impl WantedResolverService {
    pub fn new(
        pg: PgPool,
        sc: ScClient,
        tokens: Arc<TokenProvider>,
        indexing: Arc<IndexingService>,
        scanner: Arc<ScAccountScanner>,
        ai_matcher: Option<Arc<AiMatcherClient>>,
        cfg: &EnrichCrawlCfg,
    ) -> Arc<Self> {
        let interval = Duration::from_secs(cfg.interval_sec.max(60));
        Arc::new(Self {
            pg,
            sc,
            tokens,
            indexing,
            scanner,
            ai_matcher,
            interval,
        })
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(svc.interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await;
            if let Err(e) = svc.run_tick_locked().await {
                warn!(error = %e, "wanted-resolver bootstrap tick failed");
            }
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = svc.run_tick_locked().await {
                            warn!(error = %e, "wanted-resolver tick failed");
                        }
                    }
                }
            }
        });
    }

    async fn run_tick_locked(&self) -> AppResult<()> {
        // Session-level advisory lock на отдельной коннекции. На время тика
        // эта коннекция занята только удержанием лока — реальная работа
        // run_tick'а идёт через свободные коннекты из пула.
        let mut lock_conn = self.pg.acquire().await?;
        let acquired: (bool,) = sqlx::query_as("SELECT pg_try_advisory_lock($1)")
            .bind(advisory_locks::WANTED_RESOLVER_TICK)
            .fetch_one(&mut *lock_conn)
            .await?;
        if !acquired.0 {
            debug!("wanted-resolver: another instance holds the lock, skipping");
            return Ok(());
        }
        let outcome = self.run_tick().await;
        let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(advisory_locks::WANTED_RESOLVER_TICK)
            .execute(&mut *lock_conn)
            .await;
        outcome
    }

    async fn run_tick(&self) -> AppResult<()> {
        let rows = self
            .fetch_wanted(
                "SELECT wt.id, wt.title, COALESCE(a.name, ''), wt.duration_ms, wt.isrc, wt.primary_artist_id
                 FROM wanted_tracks wt
                 LEFT JOIN artists a ON a.id = wt.primary_artist_id
                 WHERE wt.status = 'wanted'
                   AND wt.track_id IS NULL
                 ORDER BY wt.updated_at NULLS FIRST
                 LIMIT $1",
                BATCH_SIZE,
                None,
            )
            .await?;
        self.process_batch(rows, None).await
    }

    pub async fn run_for_artist(&self, artist_id: Uuid, max: i64) -> AppResult<()> {
        let rows = self
            .fetch_wanted(
                "SELECT wt.id, wt.title, COALESCE(a.name, ''), wt.duration_ms, wt.isrc, wt.primary_artist_id
                 FROM wanted_tracks wt
                 LEFT JOIN artists a ON a.id = wt.primary_artist_id
                 WHERE wt.status = 'wanted'
                   AND wt.track_id IS NULL
                   AND wt.primary_artist_id = $2
                 ORDER BY wt.updated_at NULLS FIRST
                 LIMIT $1",
                max,
                Some(artist_id),
            )
            .await?;
        self.process_batch(rows, Some(artist_id)).await
    }

    async fn fetch_wanted(
        &self,
        sql: &str,
        limit: i64,
        artist_id: Option<Uuid>,
    ) -> AppResult<Vec<WantedRecord>> {
        let mut q = sqlx::query_as::<
            _,
            (
                Uuid,
                String,
                String,
                Option<i32>,
                Option<String>,
                Option<Uuid>,
            ),
        >(sql)
        .bind(limit);
        if let Some(aid) = artist_id {
            q = q.bind(aid);
        }
        let rows = q.fetch_all(&self.pg).await?;
        Ok(rows
            .into_iter()
            .filter(|(_, t, _, _, _, _)| !t.trim().is_empty())
            .map(
                |(id, title, artist, duration_ms, isrc, primary_artist_id)| WantedRecord {
                    id,
                    title,
                    artist_name: artist,
                    duration_ms,
                    isrc,
                    primary_artist_id,
                },
            )
            .collect())
    }

    async fn process_batch(
        &self,
        rows: Vec<WantedRecord>,
        ctx_artist: Option<Uuid>,
    ) -> AppResult<()> {
        if rows.is_empty() {
            return Ok(());
        }
        info!(batch = rows.len(), ?ctx_artist, "wanted-resolver tick");

        let chain = match self.tokens.chain(TokenKind::PublicPool).await {
            Ok(c) => c,
            Err(e) => {
                debug!(error = %e, "wanted-resolver: token pool unavailable");
                return Ok(());
            }
        };

        let mut linked_ids: std::collections::HashSet<Uuid> = std::collections::HashSet::new();

        // Stage 1 — listing привязанных SC аккаунтов артиста.
        // Группируем wanted'ы по артисту и за раз скармливаем сканеру.
        let mut by_artist: std::collections::HashMap<Uuid, Vec<&WantedRecord>> =
            std::collections::HashMap::new();
        for r in &rows {
            if let Some(aid) = r.primary_artist_id {
                by_artist.entry(aid).or_default().push(r);
            }
        }
        for (artist_id, group) in by_artist {
            let inputs: Vec<WantedRow> = group
                .iter()
                .map(|r| WantedRow {
                    id: r.id,
                    title: r.title.clone(),
                    artist_name: r.artist_name.clone(),
                    duration_ms: r.duration_ms,
                    isrc: r.isrc.clone(),
                })
                .collect();
            match self.scanner.scan_for_artist(artist_id, &inputs).await {
                Ok(linked) => {
                    for l in linked {
                        linked_ids.insert(l.wanted_id);
                    }
                }
                Err(e) => warn!(%artist_id, error = %e, "wanted-resolver: account scan failed"),
            }
        }

        // Stage 2 — для остальных: existing tracks + общий SC search.
        // Bounded-concurrent (SC через rotating proxy), а не серийный for{await}.
        let sem = Arc::new(Semaphore::new(STAGE2_CONCURRENCY));
        let pending: Vec<&WantedRecord> =
            rows.iter().filter(|r| !linked_ids.contains(&r.id)).collect();
        join_all(pending.into_iter().map(|r| {
            let sem = sem.clone();
            let chain = &chain;
            async move {
                let _permit = sem.acquire().await;
                match self.resolve_one(r, chain).await {
                    Ok(true) => {}
                    Ok(false) => {
                        let _ = sqlx::query(
                            "UPDATE wanted_tracks SET updated_at = now() WHERE id = $1",
                        )
                            .bind(r.id)
                            .execute(&self.pg)
                            .await;
                    }
                    Err(e) => warn!(error = %e, %r.id, "wanted-resolver: resolve_one failed"),
                }
            }
        }))
            .await;
        Ok(())
    }

    async fn resolve_one(&self, w: &WantedRecord, chain: &[String]) -> AppResult<bool> {
        // Stage A — пробуем найти трек среди уже tracks этого артиста
        // (без сетевых запросов).
        if let Some(sc_id) = self
            .try_link_via_existing(w.id, &w.title, &w.artist_name)
            .await?
        {
            link_wanted_to_sc(&self.pg, w.id, &sc_id).await?;
            info!(%w.id, sc_track_id = %sc_id, "wanted-resolver: linked via existing indexed");
            return Ok(true);
        }

        // Stage B — общий SC search по двум вариантам query.
        let candidates = self.sc_search(w, chain).await;
        if candidates.is_empty() {
            return Ok(false);
        }

        // Сначала ищем безусловный лучший. Параллельно собираем borderline-список
        // (0.45..0.7) для возможной AI-проверки.
        let mut best_strict: Option<(f32, usize)> = None;
        let mut borderline: Vec<usize> = Vec::new();
        for (idx, c) in candidates.iter().enumerate() {
            let m = evaluate_sc_candidate(
                c,
                &w.title,
                &w.artist_name,
                w.isrc.as_deref(),
                w.duration_ms,
            );
            let score = m.score();
            if score >= SEARCH_LINK_THRESHOLD {
                if best_strict
                    .as_ref()
                    .map(|(s, _)| score > *s)
                    .unwrap_or(true)
                {
                    best_strict = Some((score, idx));
                }
            } else if score >= BORDERLINE_LOW {
                borderline.push(idx);
            }
        }

        if let Some((score, idx)) = best_strict {
            return self
                .link_search_hit(w, &candidates[idx], score, "sc_search")
                .await;
        }

        if borderline.is_empty() {
            debug!(%w.id, "wanted-resolver: no SC candidate above threshold");
            return Ok(false);
        }

        // Borderline — отдаём на AI matcher (если включён).
        let Some(ai) = self.ai_matcher.as_ref() else {
            debug!(%w.id, count = borderline.len(), "wanted-resolver: borderline candidates, AI disabled");
            return Ok(false);
        };
        let ai_cands: Vec<MatchCandidate> = borderline
            .iter()
            .enumerate()
            .map(|(i, &orig_idx)| {
                let c = &candidates[orig_idx];
                MatchCandidate {
                    id: i as u32,
                    artist: c
                        .get("user")
                        .and_then(|u| u.get("username"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    title: c.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    uploader: None,
                    duration_sec: c
                        .get("duration")
                        .and_then(|v| v.as_i64())
                        .map(|ms| (ms / 1000) as i32),
                }
            })
            .collect();
        let ai_pick = ai
            .pick(
                MatchTarget {
                    artist: &w.artist_name,
                    title: &w.title,
                },
                &ai_cands,
            )
            .await?;
        let Some(pick) = ai_pick else {
            debug!(%w.id, "wanted-resolver: AI returned no match");
            return Ok(false);
        };
        let chosen = match borderline.get(pick.candidate_id as usize) {
            Some(&i) => &candidates[i],
            None => return Ok(false),
        };
        self.link_search_hit(w, chosen, pick.confidence, "sc_search+ai")
            .await
    }

    async fn link_search_hit(
        &self,
        w: &WantedRecord,
        candidate: &Value,
        score: f32,
        via: &'static str,
    ) -> AppResult<bool> {
        let Some(sc_track_id) = candidate
            .get("urn")
            .and_then(|v| v.as_str())
            .and_then(sc_track_id_from_urn)
        else {
            return Ok(false);
        };
        self.indexing
            .ingest_track_from_sc(candidate, TrackPriority::Discovery)
            .await?;
        link_wanted_to_sc(&self.pg, w.id, &sc_track_id).await?;
        info!(%w.id, score, sc_track_id, via, "wanted-resolver: linked");
        Ok(true)
    }

    async fn sc_search(&self, w: &WantedRecord, chain: &[String]) -> Vec<Value> {
        let queries: Vec<String> = if w.artist_name.is_empty() {
            vec![w.title.clone()]
        } else {
            vec![format!("{} {}", w.artist_name, w.title), w.title.clone()]
        };
        let mut out: Vec<Value> = Vec::new();
        for q in queries {
            let path = format!(
                "/tracks?q={}&access=playable&limit={}",
                urlencoding::encode(&q),
                SEARCH_LIMIT
            );
            let resp: Value = match try_with_chain(chain, |t| {
                let sc = self.sc.clone();
                let path = path.clone();
                async move { sc.api_get_value(&path, &t, None).await }
            })
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    debug!(error = %e, %w.id, "SC search failed");
                    continue;
                }
            };
            let arr: Vec<Value> = if let Some(arr) = resp.as_array() {
                arr.clone()
            } else if let Some(arr) = resp.get("collection").and_then(|v| v.as_array()) {
                arr.clone()
            } else {
                Vec::new()
            };
            if !arr.is_empty() {
                out.extend(arr);
                if out.len() >= SEARCH_LIMIT {
                    break;
                }
            }
        }
        out
    }

    async fn try_link_via_existing(
        &self,
        wanted_id: Uuid,
        title: &str,
        _artist_name: &str,
    ) -> AppResult<Option<String>> {
        let primary_artist_id: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT primary_artist_id FROM wanted_tracks WHERE id = $1")
                .bind(wanted_id)
                .fetch_optional(&self.pg)
                .await?;
        let Some((Some(artist_id),)) = primary_artist_id else {
            return Ok(None);
        };
        Ok(
            find_best_indexed_for_artist_title(&self.pg, artist_id, title)
                .await?
                .map(|m| m.sc_track_id),
        )
    }
}

/// Порог совпадения title для линковки уже-проиндексированного трека.
/// Артист матчится через `track_artists.artist_id`, поэтому планку держим
/// высокой, чтобы не залинковать одноимёнки разных треков.
pub const INDEXED_TITLE_THRESHOLD: f32 = 0.85;

#[derive(Debug, Clone)]
pub struct IndexedMatch {
    pub track_id: Uuid,
    pub sc_track_id: String,
    pub score: f32,
}

/// Ищет лучший indexed_track этого артиста по title через `matcher::title_score`.
/// Используется и artist_crawl, и wanted_resolver. Чистый pg-запрос + scoring,
/// без сетевых вызовов.
///
/// Предфильтр через `title_normalized` использует индекс `tracks_title_norm_idx`
/// и режет full-scan по тысячам треков артиста: сначала equal-match, затем
/// prefix-LIKE на первое токенное слово (по нему всё ещё gist-приемлемый
/// LIKE), и только остаток score'ится через дорогой Levenshtein.
pub async fn find_best_indexed_for_artist_title(
    pg: &PgPool,
    artist_id: Uuid,
    target_title: &str,
) -> AppResult<Option<IndexedMatch>> {
    let normalized = crate::modules::enrich::normalize::normalize_title(target_title);
    if normalized.is_empty() {
        return Ok(None);
    }
    let first_word_prefix = normalized
        .split_whitespace()
        .next()
        .map(|w| format!("{w}%"))
        .unwrap_or_else(|| format!("{normalized}%"));

    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT it.id, it.sc_track_id, COALESCE(it.title, '')
         FROM tracks it
         JOIN track_artists ta ON ta.track_id = it.id
         WHERE ta.artist_id = $1
           AND ta.role = 'primary'
           AND (it.title_normalized = $2 OR it.title_normalized LIKE $3)",
    )
    .bind(artist_id)
    .bind(&normalized)
    .bind(&first_word_prefix)
    .fetch_all(pg)
    .await?;

    let mut best: Option<IndexedMatch> = None;
    for (track_id, sc_track_id, raw_title) in rows {
        if raw_title.is_empty() {
            continue;
        }
        let s = crate::modules::enrich::matcher::title_score(target_title, &raw_title, None);
        if s < INDEXED_TITLE_THRESHOLD {
            continue;
        }
        if best.as_ref().map(|b| s > b.score).unwrap_or(true) {
            best = Some(IndexedMatch {
                track_id,
                sc_track_id,
                score: s,
            });
        }
    }
    Ok(best)
}

/// Линкует wanted_track к найденному indexed_track (по sc_track_id) и
/// перетаскивает связи с альбомами. Race-safe (UPDATE WHERE id, ON CONFLICT
/// DO NOTHING для album_tracks).
pub async fn link_wanted_to_sc(pg: &PgPool, wanted_id: Uuid, sc_track_id: &str) -> AppResult<()> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "UPDATE wanted_tracks
         SET track_id = (SELECT id FROM tracks WHERE sc_track_id = $2 LIMIT 1),
             status = 'linked',
             updated_at = now()
         WHERE id = $1
         RETURNING track_id",
    )
    .bind(wanted_id)
    .bind(sc_track_id)
    .fetch_optional(pg)
    .await?;
    let Some((Some(indexed_id),)) = row else {
        return Ok(());
    };
    let albums: Vec<(Uuid, i16)> = sqlx::query_as(
        "SELECT album_id, position FROM wanted_track_albums WHERE wanted_track_id = $1",
    )
    .bind(wanted_id)
    .fetch_all(pg)
    .await?;
    for (album_id, position) in albums {
        sqlx::query(
            "UPDATE tracks
             SET album_id = COALESCE(album_id, $2),
                 album_position = COALESCE(album_position, $3)
             WHERE id = $1",
        )
        .bind(indexed_id)
        .bind(album_id)
        .bind(position)
        .execute(pg)
        .await?;
        sqlx::query(
            "INSERT INTO album_tracks (album_id, track_id, position)
             VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(album_id)
        .bind(indexed_id)
        .bind(position)
        .execute(pg)
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct WantedRecord {
    id: Uuid,
    title: String,
    artist_name: String,
    duration_ms: Option<i32>,
    isrc: Option<String>,
    primary_artist_id: Option<Uuid>,
}
