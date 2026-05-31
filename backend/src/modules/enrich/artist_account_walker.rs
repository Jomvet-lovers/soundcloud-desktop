//! Periodic-walk по `artist_sc_accounts`: для каждого привязанного аккаунта
//! артиста подтягиваем `/users/{sc_user_id}/tracks` через client_credentials
//! пул, и ingest'им треки. На каждый new ingest'нутый — создаём `track_artists`
//! линк (role='primary') если artist соответствует через title match.
//!
//! Отдельно от `wanted_resolver` / `sc_account_scan`: те ходят по аккаунту
//! когда есть wanted-row, walker — без триггера, чтобы новые релизы
//! привязанных артистов попадали в нашу БД даже без Genius-входа.

use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::enrich::matcher::title_score;
use crate::modules::enrich::normalize::normalize_name;
use crate::modules::indexing::IndexingService;
use crate::modules::tracks::{TrackPriority, TrackRepository};
use crate::sc::ScClient;

const TICK: Duration = Duration::from_secs(15 * 60);
const PER_TICK_ARTISTS: i64 = 50;
const PER_ARTIST_PAGES: usize = 5;
const PAGE_SIZE: i64 = 100;
const COOLDOWN_HOURS: i64 = 24;
const TITLE_MATCH_THRESHOLD: f32 = 0.7;

pub struct ArtistAccountWalker {
    pg: PgPool,
    sc: ScClient,
    tokens: Arc<TokenProvider>,
    indexing: Arc<IndexingService>,
    tracks: TrackRepository,
}

impl ArtistAccountWalker {
    pub fn new(
        pg: PgPool,
        sc: ScClient,
        tokens: Arc<TokenProvider>,
        indexing: Arc<IndexingService>,
    ) -> Arc<Self> {
        let tracks = TrackRepository::new(pg.clone());
        Arc::new(Self {
            pg,
            sc,
            tokens,
            indexing,
            tracks,
        })
    }

    pub fn spawn(self: &Arc<Self>, shutdown: CancellationToken) {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    _ = ticker.tick() => {
                        if let Err(e) = me.tick().await {
                            warn!(error = %e, "artist_account_walker tick failed");
                        }
                    }
                }
            }
        });
    }

    async fn tick(&self) -> AppResult<()> {
        let artists: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id, name FROM artists \
             WHERE merged_into IS NULL \
               AND (last_account_walk_at IS NULL \
                    OR last_account_walk_at < now() - ($1::int * INTERVAL '1 hour')) \
             ORDER BY last_account_walk_at NULLS FIRST \
             LIMIT $2",
        )
        .bind(COOLDOWN_HOURS as i32)
        .bind(PER_TICK_ARTISTS)
        .fetch_all(&self.pg)
        .await?;
        if artists.is_empty() {
            return Ok(());
        }
        // Артисты независимы — обходим bounded-concurrent, а не серийно.
        let sem = Arc::new(Semaphore::new(6));
        join_all(artists.into_iter().map(|(artist_id, name)| {
            let sem = sem.clone();
            async move {
                let _permit = sem.acquire().await;
                if let Err(e) = self.walk_artist(artist_id, &name).await {
                    debug!(%artist_id, error = %e, "artist_account_walker: walk failed");
                }
                let _ =
                    sqlx::query("UPDATE artists SET last_account_walk_at = now() WHERE id = $1")
                        .bind(artist_id)
                        .execute(&self.pg)
                        .await;
            }
        }))
            .await;
        Ok(())
    }

    async fn walk_artist(&self, artist_id: Uuid, artist_name: &str) -> AppResult<()> {
        let accounts: Vec<String> = sqlx::query_as(
            "SELECT sc_user_id FROM artist_sc_accounts \
             WHERE artist_id = $1 AND role IN ('main', 'alt', 'demo')",
        )
        .bind(artist_id)
        .fetch_all(&self.pg)
        .await?
        .into_iter()
        .map(|(s,): (String,)| s)
        .collect();
        if accounts.is_empty() {
            return Ok(());
        }
        let target_n = normalize_name(artist_name);
        if target_n.is_empty() {
            return Ok(());
        }
        let chain = self.tokens.chain(TokenKind::PublicPool).await?;
        let mut new_count = 0usize;
        for sc_user_id in accounts {
            let tracks = self.fetch_user_tracks(&sc_user_id, &chain).await?;
            for tr in tracks {
                if !track_matches_artist(&tr, &target_n) {
                    continue;
                }
                let Some(sc_track_id) = tr
                    .get("urn")
                    .and_then(|v| v.as_str())
                    .map(|u| extract_sc_id(u).to_string())
                else {
                    continue;
                };
                if let Err(e) = self
                    .indexing
                    .ingest_track_from_sc(&tr, TrackPriority::Discovery)
                    .await
                {
                    debug!(error = %e, sc_track_id, "walker: ingest failed");
                    continue;
                }
                if let Some(track_row) = self.tracks.find_by_sc_track_id(&sc_track_id).await? {
                    if track_row.primary_artist_id.is_none() {
                        let _ = sqlx::query(
                            "INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence) \
                             VALUES ($1, $2, 'primary', 0, 'walker', 0.85) \
                             ON CONFLICT (track_id, artist_id, role) DO NOTHING",
                        )
                        .bind(track_row.id)
                        .bind(artist_id)
                        .execute(&self.pg)
                        .await;
                        let _ = sqlx::query(
                            "UPDATE tracks SET primary_artist_id = $2, updated_at = now() \
                             WHERE id = $1 AND primary_artist_id IS NULL",
                        )
                        .bind(track_row.id)
                        .bind(artist_id)
                        .execute(&self.pg)
                        .await;
                        new_count += 1;
                    }
                }
            }
        }
        if new_count > 0 {
            info!(%artist_id, attached = new_count, "artist_account_walker: linked");
        }
        Ok(())
    }

    async fn fetch_user_tracks(&self, sc_user_id: &str, chain: &[String]) -> AppResult<Vec<Value>> {
        let mut acc: Vec<Value> = Vec::new();
        let mut next: Option<String> = None;
        for _ in 0..PER_ARTIST_PAGES {
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
                    let path = format!("/users/{sc_user_id}/tracks");
                    let params = [
                        ("limit".into(), PAGE_SIZE.to_string()),
                        ("linked_partitioning".into(), "true".into()),
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
            let resp = match fetched {
                Ok(v) => v,
                Err(e) => {
                    debug!(sc_user_id, error = %e, "artist_account_walker: page fetch failed");
                    break;
                }
            };
            let items: Vec<Value> = resp
                .get("collection")
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            if items.is_empty() {
                break;
            }
            acc.extend(items);
            let Some(href) = resp.get("next_href").and_then(|v| v.as_str()) else {
                break;
            };
            if href.is_empty() || Some(href) == next.as_deref() {
                break;
            }
            next = Some(href.to_string());
        }
        Ok(acc)
    }
}

/// Артист считается «нашим» для этого трека если либо:
/// * uploader.username нормализуется в target_n,
/// * либо title содержит "<artist> -" префикс (классический reupload).
///
/// Этого достаточно — после ingest'а enrich-pipeline уточнит canonical.
fn track_matches_artist(track: &Value, target_n: &str) -> bool {
    let uploader = track
        .get("user")
        .and_then(|u| u.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !uploader.is_empty() && normalize_name(uploader) == target_n {
        return true;
    }
    let title = track.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title.is_empty() {
        return false;
    }
    if let Some((maybe_artist, _)) = title.split_once(" - ") {
        if normalize_name(maybe_artist) == target_n {
            return true;
        }
    }
    // Fallback: title fuzzy-match с самим артистом. Чисто запасной критерий
    // для случаев типа `Artist Name — Track Name (Free DL)` где дефис
    // нестандартный. Порог 0.7 совпадает с ACCOUNT_LINK_THRESHOLD в
    // sc_account_scan.
    title_score(target_n, title, Some(uploader)) >= TITLE_MATCH_THRESHOLD
        && normalize_name(title).contains(target_n)
}
