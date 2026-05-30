//! Vibe + lyric search: затягивающий семантический поиск поверх того же
//! проекционного слоя, что и `/search/db/*` (project_to_sc_shape +
//! enrich::dto::apply_to_tracks), но с двумя источниками кандидатов:
//!
//!   * vibe  — MuLan-вектор запроса → Qdrant tracks_clap (через готовый
//!             [`RecommendationsService::search_by_text`]: enrich_and_boost,
//!             artist_cap, take_verified);
//!   * lyrics — PG FTS по lyrics_cache (expression GIN index, mode=text) и/или bge-m3-вектор →
//!             Qdrant tracks_lyrics (mode=semantic), merge в auto.
//!
//! Энкодинг запроса кешируется в [`WorkerClient`] (длинный TTL, single-flight),
//! а ранжированные списки — здесь, в Redis на короткий TTL: vibe ~90s, lyrics
//! ~60s. Версионный токен `v1` в ключах позволяет сбросить кеш при смене модели.

use std::collections::HashMap;
use std::sync::Arc;

use qdrant_client::qdrant::SearchPointsBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tracing::debug;

use crate::cache::cache_service::CacheScope;
use crate::cache::CacheService;
use crate::error::AppResult;
use crate::modules::enrich::dto as enrich_dto;
use crate::modules::lyrics::{EncodeOutcome, WorkerClient};
use crate::modules::recommendations::RecommendationsService;
use crate::modules::tracks::{project_to_sc_shape, TrackRow};
use crate::qdrant::{collections, QdrantService};

/// Vibe-список меняется медленно (каталог + индекс), но запрос может повторяться
/// волной — 90s гасит дубли, не подмораживая свежесть.
const VIBE_RES_TTL_SECS: u64 = 90;
/// Lyrics-выдача чуть динамичнее (новые тексты приезжают пайплайном) — 60s.
const LYRICS_RES_TTL_SECS: u64 = 60;

const MAX_QUERY_LEN: usize = 128;
const VIBE_MAX_LIMIT: usize = 40;
const VIBE_DEFAULT_LIMIT: usize = 24;
const LYRICS_MAX_LIMIT: i64 = 50;
const LYRICS_DEFAULT_LIMIT: i64 = 20;
const LYRICS_MAX_PAGE: i64 = 24;
/// Потолок окна, которое `mode=auto` тянет из каждого источника на промах кэша.
/// Дедуп между FTS и Qdrant требует фетчить [0..(page+1)*limit] целиком; без
/// потолка глубокая страница тащила бы по >1000 строк FTS + точек Qdrant.
const LYRICS_AUTO_MAX_WINDOW: i64 = 200;
const TOP_GENRES: usize = 3;

/// Защитный statement_timeout на FTS-выдачу — как в `search::repository`.
const STATEMENT_TIMEOUT_MS: i32 = 2500;

/// FTS-выражение для текстового поиска по лирике. ОБЯЗАНО совпадать байт-в-байт
/// (по структуре) с expression-индексом `lyrics_cache_fts_gin` (migration 0029),
/// иначе планировщик не подхватит индекс и FTS уйдёт в seq scan. `lc` — алиас
/// lyrics_cache.
const LYRICS_FTS_EXPR: &str = "to_tsvector('simple', coalesce(lc.plain_text, '') || ' ' || regexp_replace(coalesce(lc.synced_lrc, ''), '\\[[0-9:.]+\\]', ' ', 'g'))";

pub struct VibeSearchService {
    pg: PgPool,
    cache: Arc<CacheService>,
    recommendations: Arc<RecommendationsService>,
    worker: Arc<WorkerClient>,
    qdrant: Arc<QdrantService>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Atmosphere {
    #[serde(rename = "topGenres")]
    pub top_genres: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VibeResponse {
    pub items: Vec<Value>,
    pub atmosphere: Atmosphere,
    /// "ready" | "preparing". preparing = вектор запроса ещё считается воркером
    /// (хайлоад); items пуст, фронт показывает «готовим вайб» и переспрашивает.
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsMode {
    Text,
    Semantic,
    Auto,
}

impl LyricsMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("text") => Self::Text,
            Some("semantic") => Self::Semantic,
            _ => Self::Auto,
        }
    }
    fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Semantic => "semantic",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LyricsHit {
    pub track: Value,
    #[serde(rename = "matchedLine")]
    pub matched_line: Option<String>,
    pub score: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LyricsSearchResponse {
    pub collection: Vec<LyricsHit>,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
    pub mode: String,
}

impl VibeSearchService {
    pub fn new(
        pg: PgPool,
        cache: Arc<CacheService>,
        recommendations: Arc<RecommendationsService>,
        worker: Arc<WorkerClient>,
        qdrant: Arc<QdrantService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            cache,
            recommendations,
            worker,
            qdrant,
        })
    }

    fn normalize_query(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.chars().take(MAX_QUERY_LEN).collect())
    }

    /// Generic Redis wrapper для типизированных ответов (короткий TTL, Shared).
    /// Cache hit → десериализуем T; miss → compute(), пишем в Redis только если
    /// `cache == true` (preparing / транзиентный сбой кэшировать нельзя — иначе
    /// пустышка залипнет на TTL). Битый кэш молча пере-вычисляется.
    async fn cached_typed<T, F, Fut>(&self, key: &str, ttl: u64, compute: F) -> AppResult<T>
    where
        T: Serialize + serde::de::DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output=AppResult<Cacheable<T>>>,
    {
        if let Ok(Some(raw)) = self.cache.get_raw(key).await {
            if let Ok(v) = serde_json::from_str::<T>(&raw) {
                return Ok(v);
            }
        }
        let Cacheable { value, cache } = compute().await?;
        if cache {
            if let Ok(json) = serde_json::to_string(&value) {
                let _ = self
                    .cache
                    .set_raw(key, &json, ttl, None, CacheScope::Shared, None)
                    .await;
            }
        }
        Ok(value)
    }

    // --- /search/vibe -------------------------------------------------------

    pub async fn vibe(
        &self,
        q: &str,
        limit: Option<usize>,
        languages: Option<&[String]>,
    ) -> AppResult<VibeResponse> {
        let Some(q_norm) = Self::normalize_query(q) else {
            return Ok(empty_vibe());
        };
        let limit = limit
            .unwrap_or(VIBE_DEFAULT_LIMIT)
            .clamp(1, VIBE_MAX_LIMIT);
        let lang_key = languages.map(|l| l.join(",")).unwrap_or_default();
        let key = vibe_res_key(&q_norm, limit, &lang_key);

        self.cached_typed(&key, VIBE_RES_TTL_SECS, || async {
            // Vibe-пайплайн: encode → tracks_clap → enrich_and_boost → artist_cap →
            // take_verified.
            let st = self
                .recommendations
                .search_by_text(&q_norm, limit, languages)
                .await?;
            // preparing (вектор ещё считается) и failed (сбой Qdrant) — не
            // финальные ответы, не кэшируем: иначе «готовим вайб» / пустышка
            // залипнет на VIBE_RES_TTL_SECS.
            if st.preparing {
                return Ok(Cacheable::skip(preparing_vibe()));
            }
            if st.failed {
                return Ok(Cacheable::skip(empty_vibe()));
            }

            // topGenres из enrichment-join'а search_by_text (RecommendResult.genre).
            let top_genres = top_genres_of(&st.results, TOP_GENRES);
            // Проекция в SC-shape в порядке ранжирования + apply_to_tracks.
            let sc_ids: Vec<String> = st
                .results
                .iter()
                .map(|r| crate::modules::recommendations::value_id_to_string(&r.id))
                .collect();
            let mut items = self.project_ordered(&sc_ids).await?;
            enrich_dto::apply_to_tracks(&self.pg, &mut items).await?;

            Ok(Cacheable::keep(VibeResponse {
                items,
                atmosphere: Atmosphere { top_genres },
                status: "ready".into(),
            }))
        })
            .await
    }

    // --- /search/lyrics -----------------------------------------------------

    pub async fn lyrics(
        &self,
        q: &str,
        mode: LyricsMode,
        page: Option<i64>,
        limit: Option<i64>,
    ) -> AppResult<LyricsSearchResponse> {
        let page = page.unwrap_or(0).clamp(0, LYRICS_MAX_PAGE);
        let limit = limit.unwrap_or(LYRICS_DEFAULT_LIMIT).clamp(1, LYRICS_MAX_LIMIT);
        let Some(q_norm) = Self::normalize_query(q) else {
            return Ok(empty_lyrics(page, limit, mode));
        };
        let key = lyrics_res_key(&q_norm, mode, page, limit);

        self.cached_typed(&key, LYRICS_RES_TTL_SECS, || async {
            let fetch = match mode {
                LyricsMode::Text => self.lyrics_text(&q_norm, page, limit).await?,
                LyricsMode::Semantic => self.lyrics_semantic(&q_norm, page, limit).await?,
                LyricsMode::Auto => self.lyrics_auto(&q_norm, page, limit).await?,
            };
            let collection = self.project_hit_tracks(fetch.hits).await?;
            let resp = LyricsSearchResponse {
                collection,
                page,
                page_size: limit,
                has_more: fetch.has_more,
                mode: mode.as_str().to_string(),
            };
            Ok(Cacheable {
                value: resp,
                cache: fetch.cacheable,
            })
        })
            .await
    }

    /// mode=text: PG FTS по lyrics_cache (LYRICS_FTS_EXPR), websearch_to_tsquery('simple'),
    /// ts_headline → matchedLine. JOIN на tracks даёт sc-проекцию позже.
    async fn lyrics_text(&self, q: &str, page: i64, limit: i64) -> AppResult<LyricsFetch> {
        let offset = page * limit;
        let fetch_limit = limit + 1;

        let mut tx = self.pg.begin().await?;
        sqlx::query(&format!("SET LOCAL statement_timeout = {STATEMENT_TIMEOUT_MS}"))
            .execute(&mut *tx)
            .await?;

        // ts_headline по plain_text/synced — отдаём строку с матчем, маркеры
        // настраиваем минимальные (>>/<<), вычищаем в snippet ниже. Матч-условие
        // и ts_rank используют LYRICS_FTS_EXPR — то же выражение, что в
        // expression-индексе (migration 0029), иначе seq scan.
        let sql = format!(
            "SELECT lc.sc_track_id, \
                    ts_rank({expr}, websearch_to_tsquery('simple', $1)) AS rank, \
                    ts_headline('simple', \
                        coalesce(lc.plain_text, regexp_replace(coalesce(lc.synced_lrc, ''), '\\[[0-9:.]+\\]', ' ', 'g')), \
                        websearch_to_tsquery('simple', $1), \
                        'StartSel=<<, StopSel=>>, MaxFragments=1, MaxWords=14, MinWords=3, FragmentDelimiter= … ' \
                    ) AS matched \
             FROM lyrics_cache lc \
             JOIN tracks t ON t.sc_track_id = lc.sc_track_id \
             WHERE {expr} @@ websearch_to_tsquery('simple', $1) \
               AND t.sharing = 'public' \
             ORDER BY rank DESC, lc.sc_track_id DESC \
             LIMIT $2 OFFSET $3",
            expr = LYRICS_FTS_EXPR
        );
        let rows: Vec<(String, f32, Option<String>)> = sqlx::query_as(&sql)
            .bind(q)
            .bind(fetch_limit)
            .bind(offset)
            .fetch_all(&mut *tx)
            .await?;

        tx.commit().await?;

        let has_more = rows.len() as i64 > limit;
        let hits = rows
            .into_iter()
            .take(limit as usize)
            .map(|(sc_track_id, rank, matched)| RawLyricsHit {
                sc_track_id,
                matched_line: matched.map(|m| clean_headline(&m)).filter(|s| !s.is_empty()),
                score: rank as f64,
            })
            .collect();
        // Чистый PG FTS — результат финальный, всегда кэшируем.
        Ok(LyricsFetch {
            hits,
            has_more,
            cacheable: true,
        })
    }

    /// mode=semantic: encode q (cached lyrics vector) → tracks_lyrics →
    /// sc_track_id'ы. matchedLine = null.
    async fn lyrics_semantic(&self, q: &str, page: i64, limit: i64) -> AppResult<LyricsFetch> {
        let vec = match self.worker.encode_lyrics_text(q).await? {
            EncodeOutcome::Ready(v) if !v.is_empty() => v,
            // Вектор ещё считается воркером → не финально, НЕ кэшируем (иначе
            // пустышка залипнет; в auto текстовый FTS всё равно даёт фолбэк, а
            // семантику дольём на переспросе).
            EncodeOutcome::Preparing => return Ok(LyricsFetch::empty(false)),
            // Genuine empty (у воркера нет вектора) — финально, кэшируем.
            _ => return Ok(LyricsFetch::empty(true)),
        };
        // offset+limit+1: Qdrant не делает offset нативно в этом билдере —
        // берём page*limit + limit + 1 и режем окно вручную.
        let offset = (page * limit).max(0) as usize;
        let want = offset + limit as usize + 1;

        let builder =
            SearchPointsBuilder::new(collections::TRACKS_LYRICS, vec, want as u64)
                .with_payload(true);
        let resp = match self.qdrant.raw().search_points(builder).await {
            Ok(r) => r,
            Err(e) => {
                // Транзиентный сбой Qdrant — пустой результат не финальный, НЕ
                // кэшируем, иначе переспрос после восстановления не пройдёт.
                debug!(error = %e, "lyrics semantic: qdrant search failed");
                return Ok(LyricsFetch::empty(false));
            }
        };

        let scored: Vec<RawLyricsHit> = resp
            .result
            .into_iter()
            .filter_map(|p| {
                let id = crate::modules::recommendations::point_id_to_value(p.id);
                let sc = crate::modules::recommendations::value_id_to_string(&id);
                if sc.is_empty() || sc == "null" {
                    return None;
                }
                Some(RawLyricsHit {
                    sc_track_id: sc,
                    matched_line: None,
                    score: p.score as f64,
                })
            })
            .skip(offset)
            .collect();

        let has_more = scored.len() as i64 > limit;
        Ok(LyricsFetch {
            hits: scored.into_iter().take(limit as usize).collect(),
            has_more,
            cacheable: true,
        })
    }

    /// mode=auto: text + semantic, merge dedupe по sc_track_id (text вперёд),
    /// matchedLine из text где есть.
    async fn lyrics_auto(&self, q: &str, page: i64, limit: i64) -> AppResult<LyricsFetch> {
        // Полное окно [0..(page+1)*limit] из обоих источников, мёрж, потом срез
        // страницы — иначе дедуп между источниками сломал бы постраничную
        // нарезку. Окно ограничено LYRICS_AUTO_MAX_WINDOW, чтобы глубокие
        // страницы не тащили по >1000 строк FTS + точек Qdrant на каждый промах.
        let full = ((page + 1) * limit).min(LYRICS_AUTO_MAX_WINDOW);
        let text = self.lyrics_text(q, 0, full).await?;
        let sem = self.lyrics_semantic(q, 0, full).await?;

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged: Vec<RawLyricsHit> = Vec::with_capacity(full as usize + 1);
        for h in text.hits.into_iter().chain(sem.hits.into_iter()) {
            if seen.insert(h.sc_track_id.clone()) {
                merged.push(h);
            }
        }

        let start = (page * limit) as usize;
        let has_more = merged.len() as i64 > (page + 1) * limit;
        let pageful: Vec<RawLyricsHit> = merged
            .into_iter()
            .skip(start)
            .take(limit as usize)
            .collect();
        Ok(LyricsFetch {
            hits: pageful,
            has_more,
            // text всегда финальный; кэшируемость решает семантическая половина.
            cacheable: text.cacheable && sem.cacheable,
        })
    }

    /// Грузит tracks по sc_track_id в порядке `ids`, проецирует в SC-shape.
    /// Отсутствующие пропускает.
    async fn project_ordered(&self, ids: &[String]) -> AppResult<Vec<Value>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<TrackRow> =
            sqlx::query_as("SELECT * FROM tracks WHERE sc_track_id = ANY($1) AND sharing = 'public'")
                .bind(ids)
                .fetch_all(&self.pg)
                .await?;
        let by_id: HashMap<String, TrackRow> =
            rows.into_iter().map(|r| (r.sc_track_id.clone(), r)).collect();

        let uploader_ids: Vec<String> = by_id
            .values()
            .filter_map(|r| r.uploader_sc_user_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let users = self.load_uploaders(&uploader_ids).await?;

        Ok(ids
            .iter()
            .filter_map(|id| {
                by_id.get(id).map(|row| {
                    let uploader = row
                        .uploader_sc_user_id
                        .as_deref()
                        .and_then(|uid| users.get(uid));
                    project_to_sc_shape(row, uploader)
                })
            })
            .collect())
    }

    /// Резолвит track для каждого hit'а (в исходном порядке), прогоняет
    /// apply_to_tracks (badge meta + enrichment), отбрасывает hit'ы без трека в
    /// зеркале. Возвращает финальные [`LyricsHit`].
    async fn project_hit_tracks(&self, hits: Vec<RawLyricsHit>) -> AppResult<Vec<LyricsHit>> {
        if hits.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = hits.iter().map(|h| h.sc_track_id.clone()).collect();
        let rows: Vec<TrackRow> =
            sqlx::query_as("SELECT * FROM tracks WHERE sc_track_id = ANY($1) AND sharing = 'public'")
                .bind(&ids)
                .fetch_all(&self.pg)
                .await?;
        let by_id: HashMap<String, TrackRow> =
            rows.into_iter().map(|r| (r.sc_track_id.clone(), r)).collect();

        let uploader_ids: Vec<String> = by_id
            .values()
            .filter_map(|r| r.uploader_sc_user_id.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let users = self.load_uploaders(&uploader_ids).await?;

        let mut projected: HashMap<String, Value> = HashMap::with_capacity(by_id.len());
        for (id, row) in &by_id {
            let uploader = row
                .uploader_sc_user_id
                .as_deref()
                .and_then(|uid| users.get(uid));
            projected.insert(id.clone(), project_to_sc_shape(row, uploader));
        }
        // apply_to_tracks мутирует Vec<Value> на месте — прогоняем массив,
        // раскладываем обратно по sc_track_id (через urn).
        let mut track_values: Vec<Value> = projected.into_values().collect();
        enrich_dto::apply_to_tracks(&self.pg, &mut track_values).await?;
        let mut by_sc: HashMap<String, Value> = HashMap::with_capacity(track_values.len());
        for tv in track_values {
            if let Some(id) = sc_id_of_track(&tv) {
                by_sc.insert(id, tv);
            }
        }

        Ok(hits
            .into_iter()
            .filter_map(|h| {
                by_sc.get(&h.sc_track_id).map(|tv| LyricsHit {
                    track: tv.clone(),
                    matched_line: h.matched_line,
                    score: h.score,
                })
            })
            .collect())
    }

    async fn load_uploaders(&self, ids: &[String]) -> AppResult<HashMap<String, Value>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let users: Vec<crate::modules::users::UserRow> =
            sqlx::query_as("SELECT * FROM users WHERE sc_user_id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pg)
                .await?;
        Ok(users
            .into_iter()
            .map(|u| {
                (
                    u.sc_user_id.clone(),
                    crate::modules::users::project_to_sc_shape(&u),
                )
            })
            .collect())
    }
}

/// Compute-результат + можно ли его кэшировать (preparing / транзиентный
/// сбой → нет).
struct Cacheable<T> {
    value: T,
    cache: bool,
}

impl<T> Cacheable<T> {
    fn keep(value: T) -> Self {
        Self { value, cache: true }
    }
    fn skip(value: T) -> Self {
        Self {
            value,
            cache: false,
        }
    }
}

/// Выдача одного режима лирик-поиска: хиты + has_more + кэшируемость
/// (semantic/auto при Preparing-энкоде или сбое Qdrant → false, остальное true).
struct LyricsFetch {
    hits: Vec<RawLyricsHit>,
    has_more: bool,
    cacheable: bool,
}

impl LyricsFetch {
    fn empty(cacheable: bool) -> Self {
        Self {
            hits: Vec::new(),
            has_more: false,
            cacheable,
        }
    }
}

/// Сырой hit до проекции трека: только id + матч-строка + скор.
#[derive(Debug, Clone)]
struct RawLyricsHit {
    sc_track_id: String,
    matched_line: Option<String>,
    score: f64,
}

fn sc_id_of_track(t: &Value) -> Option<String> {
    if let Some(urn) = t.get("urn").and_then(|v| v.as_str()) {
        return crate::common::sc_ids::normalize_sc_track_id(urn);
    }
    None
}

/// Чистит ts_headline-фрагмент: убирает наши маркеры `<<`/`>>`, схлопывает
/// пробелы. Возвращает компактную строку с матчем.
fn clean_headline(raw: &str) -> String {
    raw.replace("<<", "")
        .replace(">>", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn top_genres_of(items: &[crate::modules::recommendations::RecommendResult], n: usize) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for it in items {
        if let Some(g) = it.genre.as_ref() {
            let g = g.trim();
            if g.is_empty() {
                continue;
            }
            let key = g.to_string();
            let c = counts.entry(key.clone()).or_insert(0);
            if *c == 0 {
                order.push(key);
            }
            *c += 1;
        }
    }
    let mut ranked: Vec<(String, usize)> = order
        .into_iter()
        .map(|g| {
            let c = counts.get(&g).copied().unwrap_or(0);
            (g, c)
        })
        .collect();
    // Частота убыванием; при равенстве — стабильно по первому появлению.
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked.into_iter().take(n).map(|(g, _)| g).collect()
}

fn sha_key(prefix: &str, raw: &str) -> String {
    let digest = hex::encode(Sha256::digest(raw.as_bytes()));
    format!("{prefix}{digest}")
}

fn vibe_res_key(q: &str, limit: usize, languages: &str) -> String {
    sha_key("vibe:res:v1:", &format!("{q}|{limit}|{languages}"))
}

fn lyrics_res_key(q: &str, mode: LyricsMode, page: i64, limit: i64) -> String {
    sha_key(
        "lyrics:res:v1:",
        &format!("{q}|{}|{page}|{limit}", mode.as_str()),
    )
}

fn empty_lyrics(page: i64, limit: i64, mode: LyricsMode) -> LyricsSearchResponse {
    LyricsSearchResponse {
        collection: Vec::new(),
        page,
        page_size: limit,
        has_more: false,
        mode: mode.as_str().to_string(),
    }
}

fn empty_vibe() -> VibeResponse {
    VibeResponse {
        items: Vec::new(),
        atmosphere: Atmosphere {
            top_genres: Vec::new(),
        },
        status: "ready".into(),
    }
}

/// Вектор запроса ещё считается воркером: items пуст, фронт показывает
/// «готовим вайб» и переспрашивает.
fn preparing_vibe() -> VibeResponse {
    VibeResponse {
        items: Vec::new(),
        atmosphere: Atmosphere {
            top_genres: Vec::new(),
        },
        status: "preparing".into(),
    }
}
