//! Periodic-walk по `artist_sc_accounts`: для каждого привязанного аккаунта
//! артиста подтягиваем `/users/{sc_user_id}/tracks` через client_credentials
//! пул, и ingest'им треки. На каждый new ingest'нутый — создаём `track_artists`
//! линк (role='primary') если artist соответствует через title match.
//!
//! Отдельно от `wanted_resolver` / `sc_account_scan`: те ходят по аккаунту
//! когда есть wanted-row, walker — без триггера, чтобы новые релизы
//! привязанных артистов попадали в нашу БД даже без Genius-входа.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use sqlx::PgPool;
use tracing::{debug, info, warn};
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
/// How many times to re-walk an account whose `/tracks` listing came back short of
/// the authoritative `track_count`. A short-but-cursor-exhausted listing means SC
/// hid some tracks as geoblocked in the region of the relay client that served this
/// walk; a fresh walk may land on a different-region client (the pool churns by
/// score) and surface the missing ones. Union by sc id across attempts. Bounded so a
/// genuinely-unreachable region (or a stale `track_count`) can't loop forever.
const MAX_GEO_ATTEMPTS: usize = 3;

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
            let tracks = self.fetch_user_tracks_complete(&sc_user_id).await?;
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

    /// Walk an account's `/tracks`, re-walking up to [`MAX_GEO_ATTEMPTS`] times while
    /// the unioned result is short of the authoritative `track_count` AND the listing
    /// keeps ending naturally (cursor exhausted) — i.e. SC is *omitting* tracks as
    /// geoblocked in the serving region, not just paginating. Union by sc id so a
    /// later attempt on a different-region relay client adds the tracks the first
    /// region hid. Returns the unioned track values.
    async fn fetch_user_tracks_complete(&self, sc_user_id: &str) -> AppResult<Vec<Value>> {
        // Authoritative GLOBAL count (not region-filtered). The yardstick for "how many
        // SHOULD be here": the per-region listing silently drops geoblocked tracks, so
        // this is the only way to notice they're missing. None → can't judge, walk once.
        let expected = self.expected_track_count(sc_user_id).await;

        let mut by_id: HashMap<String, Value> = HashMap::new();
        for attempt in 0..MAX_GEO_ATTEMPTS {
            // Each retry rotates region: attempt 0 = best region (no preference),
            // attempt N defers the first N distinct countries so the relay serves the
            // listing from a fresh region, surfacing tracks the earlier regions hid.
            let (tracks, exhausted) = self
                .fetch_user_tracks_once(sc_user_id, attempt as i32)
                .await?;
            for tr in tracks {
                if let Some(id) = sc_id_of(&tr) {
                    by_id.entry(id).or_insert(tr);
                }
            }

            let got = by_id.len() as i64;
            match expected {
                // Reached (or beat) the global count, or the listing did NOT end
                // naturally (we hit the page cap — a depth limit, not a geoblock):
                // re-walking won't reveal more, so stop.
                Some(exp) if got >= exp || !exhausted => break,
                Some(exp) => {
                    let last = attempt + 1 == MAX_GEO_ATTEMPTS;
                    if last {
                        // Still short after retries: the missing tracks are blocked in
                        // every region we happened to reach. The walker is periodic and
                        // ingest is an idempotent upsert, so future walks keep unioning;
                        // surface the residual gap so it's observable meanwhile.
                        warn!(
                            sc_user_id,
                            expected = exp,
                            got,
                            gap = exp - got,
                            "artist_account_walker: track listing still geo-incomplete \
                             after retries — some tracks geoblocked in all reached regions"
                        );
                    } else {
                        debug!(
                            sc_user_id,
                            expected = exp,
                            got,
                            attempt,
                            "artist_account_walker: short listing, re-walking for geoblocked tracks"
                        );
                    }
                    if last {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok(by_id.into_values().collect())
    }

    /// One full cursor walk of `/users/{id}/tracks` from the region the relay picks
    /// after deferring the first `region_rotation` countries. Returns the page items and
    /// whether the cursor exhausted naturally (`true`) vs. stopped at the page cap.
    async fn fetch_user_tracks_once(
        &self,
        sc_user_id: &str,
        region_rotation: i32,
    ) -> AppResult<(Vec<Value>, bool)> {
        let path = format!("/users/{sc_user_id}/tracks");
        let mut acc: Vec<Value> = Vec::new();
        let mut cursor: Option<String> = None;
        let mut exhausted = false;
        for _ in 0..PER_ARTIST_PAGES {
            let page = match self
                .read
                .list_page_rotated(
                    TokenKind::PublicPool,
                    &path,
                    &[],
                    cursor.as_deref(),
                    PAGE_SIZE,
                    region_rotation,
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
                exhausted = true;
                break;
            }
            acc.extend(page.items);
            match page.next_href {
                Some(href) if Some(&href) != cursor.as_ref() => cursor = Some(href),
                // No next cursor (or it stopped advancing) → the listing ended.
                _ => {
                    exhausted = true;
                    break;
                }
            }
        }
        Ok((acc, exhausted))
    }

    /// Authoritative public `track_count` for the account, read off the user object
    /// (a GLOBAL, non-region-filtered figure). None if the user can't be fetched.
    async fn expected_track_count(&self, sc_user_id: &str) -> Option<i64> {
        let user = self
            .read
            .user_by_id(TokenKind::PublicPool, sc_user_id)
            .await
            .ok()?;
        user.get("track_count").and_then(Value::as_i64)
    }
}

/// Extract the bare SC track id from a track value (`urn` → numeric id), if present.
fn sc_id_of(tr: &Value) -> Option<String> {
    tr.get("urn")
        .and_then(|v| v.as_str())
        .map(|u| extract_sc_id(u).to_string())
        .filter(|s| !s.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn sc_id_of_reads_urn_and_rejects_blank() {
        assert_eq!(
            sc_id_of(&json!({ "urn": "soundcloud:tracks:636270093" })).as_deref(),
            Some("636270093")
        );
        assert_eq!(sc_id_of(&json!({ "urn": "" })), None);
        assert_eq!(sc_id_of(&json!({ "id": 1 })), None);
    }

    #[test]
    fn union_by_id_dedups_across_region_attempts() {
        // Two region-walks overlap: region A returns tracks {1,2}, region B returns
        // {2,3} (track 3 was geoblocked in A). Union by sc id must yield {1,2,3} once.
        let region_a = vec![
            json!({ "urn": "soundcloud:tracks:1" }),
            json!({ "urn": "soundcloud:tracks:2" }),
        ];
        let region_b = vec![
            json!({ "urn": "soundcloud:tracks:2" }),
            json!({ "urn": "soundcloud:tracks:3" }),
        ];
        let mut by_id: HashMap<String, Value> = HashMap::new();
        for tr in region_a.into_iter().chain(region_b) {
            if let Some(id) = sc_id_of(&tr) {
                by_id.entry(id).or_insert(tr);
            }
        }
        let mut ids: Vec<_> = by_id.keys().cloned().collect();
        ids.sort();
        assert_eq!(ids, vec!["1", "2", "3"]);
    }
}
