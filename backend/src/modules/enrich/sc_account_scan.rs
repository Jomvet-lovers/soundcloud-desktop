//! Сканер привязанных к артисту SC-аккаунтов.
//!
//! Идея: у артиста в `artist_sc_accounts` лежит список SC user_id (main / alt /
//! demo / auto_match). Перед тем как «искать по всему SC», обойти эти аккаунты
//! `/users/{urn}/tracks` и попытаться сматчить wanted_tracks этого артиста с
//! фактически залитыми треками. Это дешевле и точнее чем общий search.
//!
//! Используется wanted_resolver'ом и может вызываться из админки/ручника.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::enrich::ai_matcher::{AiMatcherClient, MatchCandidate, MatchTarget};
use crate::modules::enrich::matcher::{evaluate_sc_candidate, sc_track_id_from_urn, TrackMatch};
use crate::modules::indexing::IndexingService;
use crate::modules::tracks::TrackPriority;
use crate::sc::ScClient;

/// Минимальный композитный score, при котором мы линкуем wanted ↔ SC-кандидат
/// в рамках *листинга привязанного аккаунта*. Тут планка ниже, чем в общем
/// search, потому что аккаунт уже привязан к артисту и шанс ложного совпадения
/// существенно меньше.
const ACCOUNT_LINK_THRESHOLD: f32 = 0.7;
/// Borderline-зона для AI fallback'а в рамках листинга аккаунта.
const BORDERLINE_LOW: f32 = 0.45;

/// Пагинация SC `/users/{urn}/tracks` — сколько брать за раз и сколько страниц
/// максимум обходить, чтобы не залипнуть на гигантском канале.
const PAGE_SIZE: i64 = 100;
const MAX_PAGES: usize = 20;
/// Лёгкий троттл между страницами одного аккаунта — не сжигаем токен на одном артисте.
const PAGE_GAP: Duration = Duration::from_millis(150);

#[derive(Debug, Clone)]
pub struct WantedRow {
    pub id: Uuid,
    pub title: String,
    pub artist_name: String,
    pub duration_ms: Option<i32>,
    pub isrc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinkedTrack {
    pub wanted_id: Uuid,
}

pub struct ScAccountScanner {
    pg: PgPool,
    sc: ScClient,
    tokens: Arc<TokenProvider>,
    indexing: Arc<IndexingService>,
    ai_matcher: Option<Arc<AiMatcherClient>>,
}

impl ScAccountScanner {
    pub fn new(
        pg: PgPool,
        sc: ScClient,
        tokens: Arc<TokenProvider>,
        indexing: Arc<IndexingService>,
        ai_matcher: Option<Arc<AiMatcherClient>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            sc,
            tokens,
            indexing,
            ai_matcher,
        })
    }

    /// Прогнать сканер по всем привязанным аккаунтам артиста, пытаясь
    /// сматчить указанные wanted_tracks с реальными SC треками.
    pub async fn scan_for_artist(
        self: &Arc<Self>,
        artist_id: Uuid,
        wanted: &[WantedRow],
    ) -> AppResult<Vec<LinkedTrack>> {
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let accounts = self.fetch_accounts(artist_id).await?;
        if accounts.is_empty() {
            return Ok(Vec::new());
        }
        let chain = match self.tokens.chain(TokenKind::PublicPool).await {
            Ok(c) => c,
            Err(e) => {
                debug!(%artist_id, error = %e, "sc_account_scan: token pool unavailable");
                return Ok(Vec::new());
            }
        };

        let mut remaining: Vec<&WantedRow> = wanted.iter().collect();
        let mut linked: Vec<LinkedTrack> = Vec::new();

        for account in accounts {
            if remaining.is_empty() {
                break;
            }
            let tracks = self.fetch_account_tracks(&account.sc_user_id, &chain).await;
            if tracks.is_empty() {
                continue;
            }
            info!(
                %artist_id,
                sc_user_id = %account.sc_user_id,
                role = %account.role,
                source = %account.source,
                tracks = tracks.len(),
                pending_wanted = remaining.len(),
                "sc_account_scan: matching account tracks against wanted"
            );

            // Pass 1 — strict (>= ACCOUNT_LINK_THRESHOLD).
            let mut newly_linked: Vec<Uuid> = Vec::new();
            for cand in &tracks {
                if remaining.is_empty() {
                    break;
                }
                let Some((wid, sc_track_id, score)) = self.best_strict_match(cand, &remaining)
                else {
                    continue;
                };
                if let Err(e) = self
                    .indexing
                    .ingest_track_from_sc(cand, TrackPriority::Discovery)
                    .await
                {
                    warn!(error = %e, sc_track_id, "sc_account_scan: ingest_track_from_sc failed");
                    continue;
                }
                if let Err(e) = self.persist_link(wid, &sc_track_id).await {
                    warn!(error = %e, %wid, "sc_account_scan: persist_link failed");
                    continue;
                }
                info!(
                    %artist_id,
                    %wid,
                    sc_track_id,
                    score,
                    sc_user_id = %account.sc_user_id,
                    "sc_account_scan: linked wanted via attached account"
                );
                linked.push(LinkedTrack { wanted_id: wid });
                newly_linked.push(wid);
            }
            if !newly_linked.is_empty() {
                remaining.retain(|w| !newly_linked.contains(&w.id));
            }

            // Pass 2 — borderline через AI matcher (если включён).
            if remaining.is_empty() {
                continue;
            }
            let Some(ai) = self.ai_matcher.as_ref() else {
                continue;
            };
            let mut ai_linked: Vec<Uuid> = Vec::new();
            for w in &remaining {
                let cand_indices: Vec<usize> = tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, c)| {
                        let m: TrackMatch = evaluate_sc_candidate(
                            c,
                            &w.title,
                            &w.artist_name,
                            w.isrc.as_deref(),
                            w.duration_ms,
                        );
                        let s = m.score();
                        (BORDERLINE_LOW..ACCOUNT_LINK_THRESHOLD)
                            .contains(&s)
                            .then_some(idx)
                    })
                    .collect();
                if cand_indices.is_empty() {
                    continue;
                }
                let ai_cands: Vec<MatchCandidate> = cand_indices
                    .iter()
                    .enumerate()
                    .map(|(i, &orig)| {
                        let c = &tracks[orig];
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
                let pick = match ai
                    .pick(
                        MatchTarget {
                            artist: &w.artist_name,
                            title: &w.title,
                        },
                        &ai_cands,
                    )
                    .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(%w.id, error = %e, "sc_account_scan: AI matcher failed");
                        continue;
                    }
                };
                let Some(pick) = pick else { continue };
                let chosen = match cand_indices.get(pick.candidate_id as usize) {
                    Some(&i) => &tracks[i],
                    None => continue,
                };
                let Some(sc_track_id) = chosen
                    .get("urn")
                    .and_then(|v| v.as_str())
                    .and_then(sc_track_id_from_urn)
                else {
                    continue;
                };
                if let Err(e) = self
                    .indexing
                    .ingest_track_from_sc(chosen, TrackPriority::Discovery)
                    .await
                {
                    warn!(error = %e, sc_track_id, "sc_account_scan: ingest_track_from_sc failed (ai)");
                    continue;
                }
                if let Err(e) = self.persist_link(w.id, &sc_track_id).await {
                    warn!(error = %e, %w.id, "sc_account_scan: persist_link failed (ai)");
                    continue;
                }
                info!(
                    %artist_id,
                    %w.id,
                    sc_track_id,
                    confidence = pick.confidence,
                    sc_user_id = %account.sc_user_id,
                    "sc_account_scan: linked wanted via AI matcher"
                );
                linked.push(LinkedTrack { wanted_id: w.id });
                ai_linked.push(w.id);
            }
            if !ai_linked.is_empty() {
                remaining.retain(|w| !ai_linked.contains(&w.id));
            }
        }

        Ok(linked)
    }

    fn best_strict_match(
        &self,
        cand: &Value,
        wanted: &[&WantedRow],
    ) -> Option<(Uuid, String, f32)> {
        let urn = cand.get("urn").and_then(|v| v.as_str()).unwrap_or("");
        let sc_track_id = sc_track_id_from_urn(urn)?;
        let mut best: Option<(Uuid, f32)> = None;
        for w in wanted {
            let m: TrackMatch = evaluate_sc_candidate(
                cand,
                &w.title,
                &w.artist_name,
                w.isrc.as_deref(),
                w.duration_ms,
            );
            let score = m.score();
            if score < ACCOUNT_LINK_THRESHOLD {
                continue;
            }
            match best {
                None => best = Some((w.id, score)),
                Some((_, prev_score)) if score > prev_score => best = Some((w.id, score)),
                _ => {}
            }
        }
        let (wid, score) = best?;
        Some((wid, sc_track_id, score))
    }

    async fn fetch_accounts(&self, artist_id: Uuid) -> AppResult<Vec<AttachedAccount>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT sc_user_id, role, source
             FROM artist_sc_accounts
             WHERE artist_id = $1
             ORDER BY verified DESC,
                      CASE role
                          WHEN 'main' THEN 0
                          WHEN 'demo' THEN 1
                          WHEN 'alt'  THEN 2
                          ELSE 3
                      END",
        )
        .bind(artist_id)
        .fetch_all(&self.pg)
        .await?;
        Ok(rows
            .into_iter()
            .filter(|(id, _, _)| !id.is_empty())
            .map(|(sc_user_id, role, source)| AttachedAccount {
                sc_user_id,
                role,
                source,
            })
            .collect())
    }

    /// Идём по `/users/{urn}/tracks` через `next_href` (SC docs), ротируя
    /// chain на ban/rate-limit. Возвращаем накопленный список треков.
    async fn fetch_account_tracks(&self, sc_user_id: &str, chain: &[String]) -> Vec<Value> {
        let user_urn = format!("soundcloud:users:{sc_user_id}");
        let mut out: Vec<Value> = Vec::new();
        let mut next: Option<String> = None;
        for page_idx in 0..MAX_PAGES {
            if page_idx > 0 {
                tokio::time::sleep(PAGE_GAP).await;
            }
            let fetched: AppResult<Value> = match &next {
                Some(href) => {
                    try_with_chain(chain, |t| {
                        let sc = self.sc.clone();
                        let href = href.clone();
                        async move { sc.api_get_absolute_value(&href, &t).await }
                    })
                    .await
                }
                None => {
                    let path = format!("/users/{user_urn}/tracks");
                    let params = [
                        ("limit".to_string(), PAGE_SIZE.to_string()),
                        ("access".to_string(), "playable,preview,blocked".to_string()),
                        ("linked_partitioning".to_string(), "true".to_string()),
                    ];
                    try_with_chain(chain, |t| {
                        let sc = self.sc.clone();
                        let path = path.clone();
                        let params = params.clone();
                        async move { sc.api_get_value(&path, &t, Some(&params)).await }
                    })
                    .await
                }
            };
            let value = match fetched {
                Ok(v) => v,
                Err(e) => {
                    debug!(sc_user_id, error = %e, "sc_account_scan: page fetch failed");
                    break;
                }
            };
            let collection: Vec<Value> = if let Some(arr) = value.as_array() {
                arr.clone()
            } else if let Some(arr) = value.get("collection").and_then(|v| v.as_array()) {
                arr.clone()
            } else {
                Vec::new()
            };
            if collection.is_empty() {
                break;
            }
            out.extend(collection);

            let Some(href) = value.get("next_href").and_then(|v| v.as_str()) else {
                break;
            };
            if href.is_empty() || Some(href) == next.as_deref() {
                break;
            }
            next = Some(href.to_string());
        }
        out
    }

    async fn persist_link(&self, wanted_id: Uuid, sc_track_id: &str) -> AppResult<()> {
        crate::modules::enrich::wanted_resolver::link_wanted_to_sc(&self.pg, wanted_id, sc_track_id)
            .await
            .map(|_| ())
    }
}

#[derive(Debug, Clone)]
struct AttachedAccount {
    sc_user_id: String,
    role: String,
    source: String,
}
