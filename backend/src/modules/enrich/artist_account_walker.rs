//! Periodic-walk по `artist_sc_accounts`: для каждого привязанного аккаунта
//! артиста подтягиваем `/users/{sc_user_id}/tracks` через client_credentials
//! пул, и ingest'им треки. На каждый new ingest'нутый — создаём `track_artists`
//! линк (role='primary') если artist соответствует через title match.
//!
//! Отдельно от `wanted_resolver` / `sc_account_scan`: те ходят по аккаунту
//! когда есть wanted-row, walker — без триггера, чтобы новые релизы
//! привязанных артистов попадали в нашу БД даже без Genius-входа.

use std::sync::Arc;

use serde_json::Value;
use sqlx::PgPool;
use tracing::{debug, info};
use uuid::Uuid;

use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;
use crate::modules::auth::TokenKind;
use crate::modules::enrich::matcher::title_score;
use crate::modules::enrich::normalize::normalize_name;
use crate::modules::indexing::IndexingService;
use crate::modules::tracks::{TrackPriority, TrackRepository};
use crate::sc::ScReadService;

const PER_ARTIST_PAGES: usize = 5;
const PAGE_SIZE: i64 = 100;
const TITLE_MATCH_THRESHOLD: f32 = 0.7;

pub struct ArtistAccountWalker {
    pg: PgPool,
    read: Arc<ScReadService>,
    indexing: Arc<IndexingService>,
    tracks: TrackRepository,
}

impl ArtistAccountWalker {
    pub fn new(pg: PgPool, read: Arc<ScReadService>, indexing: Arc<IndexingService>) -> Arc<Self> {
        let tracks = TrackRepository::new(pg.clone());
        Arc::new(Self {
            pg,
            read,
            indexing,
            tracks,
        })
    }

    pub async fn walk_artist(&self, artist_id: Uuid, artist_name: &str) -> AppResult<()> {
        let accounts: Vec<String> = sqlx::query_file_scalar!(
            "queries/enrich/artist_account_walker/list_accounts.sql",
            artist_id
        )
        .fetch_all(&self.pg)
        .await?;
        if accounts.is_empty() {
            return Ok(());
        }
        let target_n = normalize_name(artist_name);
        if target_n.is_empty() {
            return Ok(());
        }
        let mut new_count = 0usize;
        let mut avatar: Option<String> = None;
        for sc_user_id in accounts {
            let tracks = self.fetch_user_tracks(&sc_user_id).await?;
            for tr in tracks {
                if avatar.is_none() {
                    if let Some(a) = tr
                        .get("user")
                        .and_then(|u| u.get("avatar_url"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        avatar = Some(a.replace("-large.", "-t500x500."));
                    }
                }
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
                // Лейбловая мета знает авторов: если она живая и артиста в ней
                // нет — это чужой трек на его аккаунте (компиляция, OST-залив).
                // Трек уже ingest'нут, но primary не присваиваем — пусть решает
                // enrich-pipeline.
                if !meta_allows_artist(&tr, artist_name) {
                    continue;
                }
                if let Some(track_row) = self.tracks.find_by_sc_track_id(&sc_track_id).await? {
                    if track_row.primary_artist_id.is_none() {
                        let _ = sqlx::query_file!(
                            "queries/enrich/artist_account_walker/insert_track_artist.sql",
                            track_row.id,
                            artist_id
                        )
                        .execute(&self.pg)
                        .await;
                        let _ = sqlx::query_file!(
                            "queries/enrich/artist_account_walker/set_primary_artist.sql",
                            track_row.id,
                            artist_id
                        )
                        .execute(&self.pg)
                        .await;
                        new_count += 1;
                    }
                }
            }
        }
        if let Some(a) = avatar {
            let _ = sqlx::query_file!(
                "queries/enrich/artist_account_walker/set_avatar.sql",
                artist_id,
                &a
            )
            .execute(&self.pg)
            .await;
        }
        if new_count > 0 {
            info!(%artist_id, attached = new_count, "artist_account_walker: linked");
        }
        Ok(())
    }

    async fn fetch_user_tracks(&self, sc_user_id: &str) -> AppResult<Vec<Value>> {
        let path = format!("/users/{sc_user_id}/tracks");
        let mut acc: Vec<Value> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..PER_ARTIST_PAGES {
            let page = match self
                .read
                .list_page(
                    TokenKind::PublicPool,
                    &path,
                    &[],
                    cursor.as_deref(),
                    PAGE_SIZE,
                )
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    debug!(sc_user_id, error = %e, "artist_account_walker: page fetch failed");
                    break;
                }
            };
            if page.items.is_empty() {
                break;
            }
            acc.extend(page.items);
            match page.next_href {
                Some(href) if Some(&href) != cursor.as_ref() => cursor = Some(href),
                _ => break,
            }
        }
        Ok(acc)
    }
}

/// Мета пуста/мусорная → не мешаем. Живая мета должна знать артиста, иначе
/// трек на его аккаунте — чужой (компиляция, OST, диджейский залив).
fn meta_allows_artist(track: &Value, artist_name: &str) -> bool {
    let Some(meta) = track.get("metadata_artist").and_then(|v| v.as_str()) else {
        return true;
    };
    let names = crate::modules::enrich::artist_names::meta_artist_names(meta);
    names.is_empty()
        || crate::modules::enrich::artist_names::name_in(
            artist_name,
            names.iter().map(|s| s.as_str()),
        )
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
