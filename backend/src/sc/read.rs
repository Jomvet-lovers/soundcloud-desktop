//! `ScReadService` — the single facade for PUBLIC SoundCloud reads.
//!
//! Every public read runs a 3-tier state chain, going to the next on failure:
//!   A) apiv2 via the relay (signed Lua method on an edge client, its own anonymous
//!      client_id + residential IP, zero OAuth budget) — the primary,
//!   B) apiv2 via proxy&relay (a scraped anonymous client_id over our infra) — token-free,
//!   C) apiv1 via an OAuth token (the legacy path) — terminal fallback.
//! A and B are combined with B+C as a hedge by default (`CALL_FETCH_STRATEGY`); a per-A
//! circuit breaker routes straight to B+C while the relay is being banned. The result is
//! always apiv1-normalized JSON so persistence is unchanged.
//!
//! Private/owner `/me/*` reads and all writes do NOT come here — they stay on apiv1 with
//! the user's own OAuth token (anonymous apiv2 cannot see private content).
//!
//! Single-entity ops own the full A→B→C chain. Paginated ops (`collection_page`,
//! `search_page`) own apiv2 only (A hedged with B): an apiv2 cursor is not valid on
//! apiv1, so a sequence must stay on one cursor space — the caller starts an apiv1
//! sequence itself when apiv2 can't even begin.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tracing::debug;

use crate::error::{AppError, AppResult};
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::sc::apiv2::Apiv2Proxy;
use crate::sc::mapping::{self, PublicCollection, SearchType};
use crate::sc::{hedge, race, ChannelHealth, FetchStrategy, ScClient};

/// Relay head start before the apiv2-proxy/apiv1 backup is hedged in. Long enough that a
/// healthy relay answers alone (1x SC load), short enough not to stall on a dead relay.
const HEDGE_DELAY: Duration = Duration::from_millis(700);

/// One apiv2 page of a collection: normalized bare entities + the next cursor.
pub struct ScCollectionPage {
    pub items: Vec<Value>,
    pub next_href: Option<String>,
}

/// One apiv2 page of a search: normalized entities + cursor.
pub struct ScSearchPage {
    pub items: Vec<Value>,
    pub next_href: Option<String>,
}

pub struct ScReadService {
    sc: ScClient,
    proxy: Apiv2Proxy,
    tokens: Arc<TokenProvider>,
    lua_health: ChannelHealth,
    strategy: FetchStrategy,
}

impl ScReadService {
    pub fn new(sc: ScClient, tokens: Arc<TokenProvider>) -> Arc<Self> {
        let proxy = Apiv2Proxy::new(sc.clone());
        Arc::new(Self {
            sc,
            proxy,
            tokens,
            lua_health: ChannelHealth::default(),
            strategy: FetchStrategy::from_env(),
        })
    }

    // ---- single-entity ops (full A→B→C) ----------------------------------------

    /// `/resolve` for a public permalink URL.
    pub async fn resolve(&self, kind: TokenKind, url: &str) -> AppResult<Value> {
        self.run(self.resolve_lua(url), self.resolve_chain(kind, url))
            .await
    }

    /// apiv2 `/tracks/{id}` (recovers full_duration etc.).
    pub async fn track_by_id(&self, kind: TokenKind, sc_track_id: &str) -> AppResult<Value> {
        self.run(
            self.entity_lua(self.sc.track_by_id_via_relay(sc_track_id)),
            self.track_chain(kind, sc_track_id),
        )
        .await
    }

    /// apiv2 `/tracks/{id}` for the duration_resolver cron (public-pool tokens for C).
    pub async fn fetch_track_v2(&self, sc_track_id: &str) -> AppResult<Value> {
        self.track_by_id(TokenKind::PublicPool, sc_track_id).await
    }

    /// apiv2 `/users/{id}` (token-free public profile). `id` is the bare numeric id.
    pub async fn user_by_id(&self, kind: TokenKind, user_id: &str) -> AppResult<Value> {
        self.run(
            self.entity_lua(self.sc.user_by_id_via_relay(user_id)),
            self.user_chain(kind, user_id),
        )
        .await
    }

    /// Playlist metadata only (full A→B→C — meta is a single GET on every channel).
    /// `playlist_id` is the bare numeric id.
    pub async fn playlist_meta(&self, kind: TokenKind, playlist_id: &str) -> AppResult<Value> {
        self.run(
            self.entity_lua(self.sc.playlist_full_via_relay(playlist_id, false)),
            self.playlist_meta_chain(kind, playlist_id),
        )
        .await
    }

    /// A playlist's full, ordered, hydrated track list via apiv2 only (A hedged with B).
    /// Errs when apiv2 can't serve it — the caller then uses its apiv1 `/tracks`
    /// pagination (apiv1's large-playlist track list needs paging, which channel C of a
    /// single GET can't give).
    pub async fn playlist_tracks(&self, playlist_id: &str) -> AppResult<Vec<Value>> {
        let pl = self
            .run(
                self.entity_lua(self.sc.playlist_full_via_relay(playlist_id, true)),
                self.proxy_playlist_hydrated(playlist_id),
            )
            .await?;
        Ok(pl
            .get("tracks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    // ---- paginated ops (apiv2 only: A hedged with B) ---------------------------

    /// One apiv2 page of a public per-user collection. Errs when apiv2 (A and B) can't
    /// serve the page — the caller then starts an apiv1 sequence from the top.
    pub async fn collection_page(
        &self,
        coll: PublicCollection,
        user_id: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<ScCollectionPage> {
        self.run(
            self.collection_lua(coll, user_id, cursor, limit),
            self.collection_proxy(coll, user_id, cursor, limit),
        )
        .await
    }

    /// Fetch a whole public per-user collection via apiv2 (each page A hedged with B),
    /// following `next_href`. Returns `(items, complete)` — `complete=false` flags a
    /// truncated/looped sequence so the caller won't delete local rows from it (matching
    /// the apiv1 `fetch_all_pages` contract). Errs only when the FIRST page fails on both
    /// channels, so the caller can fall back to an apiv1 sequence.
    pub async fn collection_all(
        &self,
        coll: PublicCollection,
        user_id: &str,
        limit: i64,
    ) -> AppResult<(Vec<Value>, bool)> {
        let mut acc: Vec<Value> = Vec::new();
        let mut cursor: Option<String> = None;
        let complete = loop {
            let page = match self
                .collection_page(coll, user_id, cursor.as_deref(), limit)
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    if acc.is_empty() {
                        return Err(e);
                    }
                    break false; // mid-sequence failure → incomplete snapshot
                }
            };
            if page.items.is_empty() {
                break cursor.is_none(); // empty first page = legit empty; empty mid = break
            }
            let full_page = page.items.len() as i64 == limit;
            acc.extend(page.items);
            match page.next_href {
                None => break !full_page,
                Some(href) if Some(&href) == cursor.as_ref() => break false,
                Some(href) => cursor = Some(href),
            }
        };
        Ok((acc, complete))
    }

    /// One apiv2 page of a typed search.
    pub async fn search_page(
        &self,
        ty: SearchType,
        q: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<ScSearchPage> {
        self.run(
            self.search_lua(ty, q, cursor, limit),
            self.search_proxy(ty, q, cursor, limit),
        )
        .await
    }

    // ---- orchestration ---------------------------------------------------------

    /// Compose channel A (relay/Lua, which records breaker health itself) with a backup
    /// per `FetchStrategy`. Breaker open / no relay → backup alone.
    async fn run<T>(
        &self,
        lua: impl std::future::Future<Output = AppResult<T>>,
        backup: impl std::future::Future<Output = AppResult<T>>,
    ) -> AppResult<T> {
        if !self.sc.has_relay() || self.lua_health.is_open() {
            return backup.await;
        }
        match self.strategy {
            FetchStrategy::Fallback => match lua.await {
                Ok(v) => Ok(v),
                Err(_) => backup.await,
            },
            FetchStrategy::Hedge => hedge(lua, HEDGE_DELAY, backup).await,
            FetchStrategy::Race => race(lua, backup).await,
        }
    }

    /// Wrap a relay accessor (`Option<Value>` of a normalized-able entity) as channel A:
    /// Some → normalize + record_ok; None → record_ban + Err (so the backup runs).
    async fn entity_lua(
        &self,
        fut: impl std::future::Future<Output = Option<Value>>,
    ) -> AppResult<Value> {
        match fut.await {
            Some(mut v) => {
                mapping::normalize_v2_to_v1(&mut v);
                self.lua_health.record_ok();
                Ok(v)
            }
            None => {
                self.lua_health.record_ban();
                Err(AppError::ScUnreachable("relay: no result".into()))
            }
        }
    }

    async fn resolve_lua(&self, url: &str) -> AppResult<Value> {
        self.entity_lua(self.sc.resolve_track_via_relay(url)).await
    }

    async fn collection_lua(
        &self,
        coll: PublicCollection,
        user_id: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<ScCollectionPage> {
        match self
            .sc
            .user_collection_via_relay(user_id, coll.lua_kind(), cursor, limit)
            .await
        {
            Some(v) => {
                self.lua_health.record_ok();
                Ok(page_from_lua(&v))
            }
            None => {
                self.lua_health.record_ban();
                Err(AppError::ScUnreachable(
                    "relay collection: no result".into(),
                ))
            }
        }
    }

    async fn search_lua(
        &self,
        ty: SearchType,
        q: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<ScSearchPage> {
        match self
            .sc
            .search_via_relay(ty.as_str(), q, cursor, limit)
            .await
        {
            Some(v) => {
                self.lua_health.record_ok();
                let page = page_from_lua(&v);
                Ok(ScSearchPage {
                    items: page.items,
                    next_href: page.next_href,
                })
            }
            None => {
                self.lua_health.record_ban();
                Err(AppError::ScUnreachable("relay search: no result".into()))
            }
        }
    }

    // ---- backups: channel B (apiv2-proxy) then, for single entities, channel C --

    async fn resolve_chain(&self, kind: TokenKind, url: &str) -> AppResult<Value> {
        match self.proxy.resolve(url).await {
            Ok(mut v) => {
                mapping::normalize_v2_to_v1(&mut v);
                Ok(v)
            }
            Err(e) => {
                debug!(error = %e, "[read] apiv2-proxy resolve failed, apiv1 fallback");
                let params = [("url".to_string(), url.to_string())];
                self.apiv1_get(kind, "/resolve", Some(&params)).await
            }
        }
    }

    async fn track_chain(&self, kind: TokenKind, id: &str) -> AppResult<Value> {
        match self.proxy.track(id).await {
            Ok(mut v) => {
                mapping::normalize_v2_to_v1(&mut v);
                Ok(v)
            }
            Err(_) => self.apiv1_get(kind, &format!("/tracks/{id}"), None).await,
        }
    }

    async fn user_chain(&self, kind: TokenKind, id: &str) -> AppResult<Value> {
        match self.proxy.user(id).await {
            Ok(mut v) => {
                mapping::normalize_v2_to_v1(&mut v);
                Ok(v)
            }
            Err(_) => self.apiv1_get(kind, &format!("/users/{id}"), None).await,
        }
    }

    async fn playlist_meta_chain(&self, kind: TokenKind, id: &str) -> AppResult<Value> {
        match self.proxy.playlist(id, false).await {
            Ok(v) => Ok(v),
            Err(_) => {
                self.apiv1_get(kind, &format!("/playlists/{id}"), None)
                    .await
            }
        }
    }

    /// Channel B for the playlist track list (hydrated apiv2). No apiv1 fallback — the
    /// caller owns the apiv1 `/tracks` pagination.
    async fn proxy_playlist_hydrated(&self, id: &str) -> AppResult<Value> {
        self.proxy.playlist(id, true).await
    }

    async fn collection_proxy(
        &self,
        coll: PublicCollection,
        user_id: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<ScCollectionPage> {
        let page = self
            .proxy
            .collection_page(coll, user_id, cursor, limit)
            .await?;
        Ok(ScCollectionPage {
            items: mapping::unwrap_collection_items(&page.items, coll),
            next_href: page.next_href,
        })
    }

    async fn search_proxy(
        &self,
        ty: SearchType,
        q: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<ScSearchPage> {
        let page = self.proxy.search_page(ty, q, cursor, limit).await?;
        let mut items = page.items;
        for it in items.iter_mut() {
            mapping::normalize_v2_to_v1(it);
        }
        Ok(ScSearchPage {
            items,
            next_href: page.next_href,
        })
    }

    /// Channel C: apiv1 GET with the OAuth token chain for `kind`, rotating on
    /// auth/ban. Lazily resolved so the happy path (A/B) never touches the token pool.
    async fn apiv1_get(
        &self,
        kind: TokenKind,
        path: &str,
        params: Option<&[(String, String)]>,
    ) -> AppResult<Value> {
        let chain = self.tokens.chain(kind).await?;
        try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = path.to_string();
            let params = params.map(<[_]>::to_vec);
            async move { sc.api_get_value(&path, &tok, params.as_deref()).await }
        })
        .await
    }
}

/// Build a page from a Lua collection result `{ collection, next_href }`. The Lua already
/// unwrapped like-feeds, so items are bare; normalize each to apiv1 shape.
fn page_from_lua(v: &Value) -> ScCollectionPage {
    let items = v
        .get("collection")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .cloned()
                .map(|mut e| {
                    mapping::normalize_v2_to_v1(&mut e);
                    e
                })
                .collect()
        })
        .unwrap_or_default();
    let next_href = v
        .get("next_href")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    ScCollectionPage { items, next_href }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn page_from_lua_normalizes_items_and_reads_cursor() {
        // Lua already unwrapped the like-feed; items are bare but un-normalized.
        let v = json!({
            "ok": true,
            "collection": [
                {"id": 1, "kind": "track", "likes_count": 5},
                {"id": 2, "kind": "track", "likes_count": 0, "urn": "soundcloud:tracks:2"}
            ],
            "next_href": "https://api-v2.soundcloud.com/users/9/track_likes?offset=z"
        });
        let p = page_from_lua(&v);
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0]["urn"], "soundcloud:tracks:1"); // synthesized
        assert_eq!(p.items[0]["favoritings_count"], 5); // aliased
        assert_eq!(p.items[1]["urn"], "soundcloud:tracks:2"); // preserved
        assert_eq!(
            p.next_href.as_deref(),
            Some("https://api-v2.soundcloud.com/users/9/track_likes?offset=z")
        );
    }

    #[test]
    fn page_from_lua_empty_cursor_is_none() {
        let v = json!({"ok": true, "collection": [], "next_href": ""});
        let p = page_from_lua(&v);
        assert!(p.items.is_empty());
        assert!(p.next_href.is_none());
    }
}
