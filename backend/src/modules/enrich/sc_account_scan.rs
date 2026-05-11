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
use crate::modules::enrich::ai_matcher::{AiMatcherClient, MatchCandidate, MatchTarget};
use crate::modules::enrich::matcher::{evaluate_sc_candidate, sc_track_id_from_urn, TrackMatch};
use crate::modules::enrich::token_pool::TokenPool;
use crate::modules::indexing::IndexingService;
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

/// Сколько SC-токенов запросить из пула.
const TOKEN_BACKGROUND_LIMIT: usize = 2;

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
    tokens: Arc<TokenPool>,
    indexing: Arc<IndexingService>,
    ai_matcher: Option<Arc<AiMatcherClient>>,
}

impl ScAccountScanner {
    pub fn new(
        pg: PgPool,
        sc: ScClient,
        tokens: Arc<TokenPool>,
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
        let token = match self.pick_token().await {
            Some(t) => t,
            None => {
                debug!(%artist_id, "sc_account_scan: no SC tokens available");
                return Ok(Vec::new());
            }
        };

        let mut remaining: Vec<&WantedRow> = wanted.iter().collect();
        let mut linked: Vec<LinkedTrack> = Vec::new();

        for account in accounts {
            if remaining.is_empty() {
                break;
            }
            let tracks = self.fetch_account_tracks(&account.sc_user_id, &token).await;
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
                let Some((wid, sc_track_id, score)) =
                    self.best_strict_match(cand, &remaining)
                else {
                    continue;
                };
                if let Err(e) = self.indexing.ensure_track_indexed(cand).await {
                    warn!(error = %e, sc_track_id, "sc_account_scan: ensure_track_indexed failed");
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
                        (s >= BORDERLINE_LOW && s < ACCOUNT_LINK_THRESHOLD).then_some(idx)
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
                if let Err(e) = self.indexing.ensure_track_indexed(chosen).await {
                    warn!(error = %e, sc_track_id, "sc_account_scan: ensure_track_indexed failed (ai)");
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

    async fn pick_token(&self) -> Option<String> {
        match self.tokens.pick_for_background(TOKEN_BACKGROUND_LIMIT).await {
            Ok(v) => v.into_iter().next(),
            Err(e) => {
                debug!(error = %e, "sc_account_scan: token pool error");
                None
            }
        }
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

    async fn fetch_account_tracks(&self, sc_user_id: &str, token: &str) -> Vec<Value> {
        let user_urn = format!("soundcloud:users:{sc_user_id}");
        let mut out: Vec<Value> = Vec::new();
        let mut offset: i64 = 0;
        for _ in 0..MAX_PAGES {
            let path = format!("/users/{user_urn}/tracks");
            let params = [
                ("limit".to_string(), PAGE_SIZE.to_string()),
                ("offset".to_string(), offset.to_string()),
                ("access".to_string(), "playable,preview,blocked".to_string()),
                ("linked_partitioning".to_string(), "1".to_string()),
            ];
            let value = match self.sc.api_get_value(&path, token, Some(&params)).await {
                Ok(v) => v,
                Err(e) => {
                    debug!(sc_user_id, offset, error = %e, "sc_account_scan: list tracks failed");
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
            let count = collection.len() as i64;
            out.extend(collection);

            let has_next = value
                .get("next_href")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !has_next || count < PAGE_SIZE {
                break;
            }
            offset += count;
            // лёгкий троттл, чтобы не сжечь токен на одном артисте
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        out
    }

    async fn persist_link(&self, wanted_id: Uuid, sc_track_id: &str) -> AppResult<()> {
        crate::modules::enrich::wanted_resolver::link_wanted_to_sc(
            &self.pg,
            wanted_id,
            sc_track_id,
        )
        .await
    }
}

#[derive(Debug, Clone)]
struct AttachedAccount {
    sc_user_id: String,
    role: String,
    source: String,
}
