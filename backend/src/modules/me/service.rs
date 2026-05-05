use std::sync::Arc;

use serde_json::{json, Value};
use tracing::debug;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, extract_sc_cursor, FetchChunkResult, GetPageOptions, ListCacheService,
    ListPageResult,
};
use crate::error::AppResult;
use crate::modules::events::EventsService;
use crate::modules::local_likes::LocalLikesService;
use crate::sc::ScClient;

const TTL_FEED: u64 = 60;
const TTL_LIKES_TRACKS: u64 = 1800;
const TTL_LIKES_PLAYLISTS: u64 = 1800;
const TTL_FOLLOWINGS: u64 = 3600;
const TTL_FOLLOWINGS_TRACKS: u64 = 60;
const TTL_FOLLOWERS: u64 = 600;
const TTL_PLAYLISTS: u64 = 3600;
const TTL_TRACKS: u64 = 120;

pub struct MeService {
    sc: ScClient,
    list_cache: Arc<ListCacheService>,
    local_likes: Arc<LocalLikesService>,
    events: Arc<EventsService>,
}

impl MeService {
    pub fn new(
        sc: ScClient,
        list_cache: Arc<ListCacheService>,
        local_likes: Arc<LocalLikesService>,
        events: Arc<EventsService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            list_cache,
            local_likes,
            events,
        })
    }

    pub async fn get_profile(&self, token: &str) -> AppResult<Value> {
        self.sc.api_get_value("/me", token, None).await
    }

    async fn apply_local_like_flags(
        &self,
        sc_user_id: &str,
        tracks: &mut [Value],
    ) -> AppResult<()> {
        let urns: Vec<String> = tracks
            .iter()
            .filter_map(|t| t.get("urn").and_then(|v| v.as_str()).map(String::from))
            .collect();
        if urns.is_empty() {
            return Ok(());
        }
        let liked = self
            .local_likes
            .get_liked_track_ids(sc_user_id, &urns)
            .await?;
        if liked.is_empty() {
            return Ok(());
        }
        for t in tracks.iter_mut() {
            if let Some(urn) = t.get("urn").and_then(|v| v.as_str()) {
                if liked.contains(urn) {
                    if let Some(obj) = t.as_object_mut() {
                        obj.insert("user_favorite".into(), Value::Bool(true));
                    }
                }
            }
        }
        Ok(())
    }

    async fn apply_local_like_flags_to_activities(
        &self,
        sc_user_id: &str,
        activities: &mut [Value],
    ) -> AppResult<()> {
        // Собираем origins с kind=track.
        let mut track_origins: Vec<Value> = activities
            .iter()
            .filter_map(|a| a.get("origin"))
            .filter(|o| o.get("kind").and_then(|k| k.as_str()) == Some("track"))
            .cloned()
            .collect();
        if track_origins.is_empty() {
            return Ok(());
        }
        self.apply_local_like_flags(sc_user_id, &mut track_origins)
            .await?;

        // by_urn: urn -> annotated track
        let by_urn: std::collections::HashMap<String, Value> = track_origins
            .into_iter()
            .filter_map(|t| {
                t.get("urn")
                    .and_then(|v| v.as_str())
                    .map(|u| (u.to_string(), t.clone()))
            })
            .collect();

        for a in activities.iter_mut() {
            let Some(origin) = a.get("origin") else {
                continue;
            };
            if origin.get("kind").and_then(|k| k.as_str()) != Some("track") {
                continue;
            }
            let Some(urn) = origin.get("urn").and_then(|v| v.as_str()) else {
                continue;
            };
            if let Some(annotated) = by_urn.get(urn) {
                if let Some(obj) = a.as_object_mut() {
                    obj.insert("origin".into(), annotated.clone());
                }
            }
        }
        Ok(())
    }

    async fn list_page(
        &self,
        cache_key: &str,
        ttl: u64,
        session_id: &str,
        page: i64,
        limit: i64,
        path: String,
        token: String,
        extra_params: Vec<(String, String)>,
    ) -> AppResult<ListPageResult<Value>> {
        let sc = self.sc.clone();
        self.list_cache
            .get_page::<Value, _, _>(
                GetPageOptions {
                    key: cache_key,
                    scope: CacheScope::User,
                    session_id: Some(session_id),
                    ttl_sec: ttl,
                    page,
                    limit,
                    chunk_size: None,
                },
                |cursor, chunk_size| {
                    let sc = sc.clone();
                    let path = path.clone();
                    let token = token.clone();
                    let extra = extra_params.clone();
                    async move {
                        let mut params: Vec<(String, String)> = extra;
                        params.push(("limit".into(), chunk_size.to_string()));
                        params.push(("linked_partitioning".into(), "true".into()));
                        if let Some(c) = cursor {
                            params.push(("cursor".into(), c));
                        }
                        let resp: Value = sc.api_get_value(&path, &token, Some(&params)).await?;
                        let items: Vec<Value> = resp
                            .get("collection")
                            .and_then(|v| v.as_array().cloned())
                            .unwrap_or_default();
                        let next_cursor = resp
                            .get("next_href")
                            .and_then(|v| v.as_str())
                            .and_then(|h| extract_sc_cursor(Some(h)));
                        Ok::<_, crate::error::AppError>(FetchChunkResult { items, next_cursor })
                    }
                },
            )
            .await
    }

    pub async fn get_feed(
        &self,
        token: &str,
        session_id: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let mut result = self
            .list_page(
                "me-feed",
                TTL_FEED,
                session_id,
                page,
                limit,
                "/me/feed".into(),
                token.to_string(),
                vec![],
            )
            .await?;
        self.apply_local_like_flags_to_activities(sc_user_id, &mut result.collection)
            .await?;
        Ok(result)
    }

    pub async fn get_feed_tracks(
        &self,
        token: &str,
        session_id: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let mut result = self
            .list_page(
                "me-feed-tracks",
                TTL_FEED,
                session_id,
                page,
                limit,
                "/me/feed/tracks".into(),
                token.to_string(),
                vec![],
            )
            .await?;
        self.apply_local_like_flags_to_activities(sc_user_id, &mut result.collection)
            .await?;
        Ok(result)
    }

    pub async fn get_liked_tracks(
        &self,
        token: &str,
        session_id: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
        access: &str,
    ) -> AppResult<ListPageResult<Value>> {
        let cache_key = build_list_cache_key("me-liked-tracks", &[("access", access.to_string())]);
        let mut result = self
            .list_page(
                &cache_key,
                TTL_LIKES_TRACKS,
                session_id,
                page,
                limit,
                "/me/likes/tracks".into(),
                token.to_string(),
                vec![("access".into(), access.to_string())],
            )
            .await?;

        if page == 0 {
            let local = self.local_likes.find_all(sc_user_id, 200, None).await?;
            if !local.collection.is_empty() {
                let existing: std::collections::HashSet<String> = result
                    .collection
                    .iter()
                    .filter_map(|t| t.get("urn").and_then(|v| v.as_str()).map(String::from))
                    .collect();
                let mut local_tracks: Vec<Value> = local
                    .collection
                    .into_iter()
                    .filter(|t| {
                        t.get("urn")
                            .and_then(|v| v.as_str())
                            .map(|u| !existing.contains(u))
                            .unwrap_or(false)
                    })
                    .collect();
                if !local_tracks.is_empty() {
                    local_tracks.extend(result.collection);
                    result.collection = local_tracks;
                }
            }
        }

        // Fire-and-forget seed likes taste.
        let events = self.events.clone();
        let sc_user_id = sc_user_id.to_string();
        let urns: Vec<String> = result
            .collection
            .iter()
            .filter_map(|t| t.get("urn").and_then(|v| v.as_str()).map(String::from))
            .collect();
        tokio::spawn(async move {
            if let Err(e) = events.ensure_likes_recorded(&sc_user_id, &urns).await {
                debug!(error = %e, "seedLikesTaste failed");
            }
        });

        Ok(result)
    }

    pub async fn get_liked_playlists(
        &self,
        token: &str,
        session_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            "me-liked-playlists",
            TTL_LIKES_PLAYLISTS,
            session_id,
            page,
            limit,
            "/me/likes/playlists".into(),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_followings(
        &self,
        token: &str,
        session_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            "me-followings",
            TTL_FOLLOWINGS,
            session_id,
            page,
            limit,
            "/me/followings".into(),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_followings_tracks(
        &self,
        token: &str,
        session_id: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let mut result = self
            .list_page(
                "me-followings-tracks",
                TTL_FOLLOWINGS_TRACKS,
                session_id,
                page,
                limit,
                "/me/followings/tracks".into(),
                token.to_string(),
                vec![],
            )
            .await?;
        self.apply_local_like_flags(sc_user_id, &mut result.collection)
            .await?;
        Ok(result)
    }

    pub async fn follow_user(&self, token: &str, user_urn: &str) -> AppResult<Value> {
        self.sc
            .api_put_value(&format!("/me/followings/{user_urn}"), token, None)
            .await
    }

    pub async fn unfollow_user(&self, token: &str, user_urn: &str) -> AppResult<Value> {
        self.sc
            .api_delete(&format!("/me/followings/{user_urn}"), token)
            .await
    }

    pub async fn get_followers(
        &self,
        token: &str,
        session_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            "me-followers",
            TTL_FOLLOWERS,
            session_id,
            page,
            limit,
            "/me/followers".into(),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_playlists(
        &self,
        token: &str,
        session_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            "me-playlists",
            TTL_PLAYLISTS,
            session_id,
            page,
            limit,
            "/me/playlists".into(),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_tracks(
        &self,
        token: &str,
        session_id: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let mut result = self
            .list_page(
                "me-tracks",
                TTL_TRACKS,
                session_id,
                page,
                limit,
                "/me/tracks".into(),
                token.to_string(),
                vec![],
            )
            .await?;
        self.apply_local_like_flags(sc_user_id, &mut result.collection)
            .await?;
        Ok(result)
    }
}

/// `{ premium: bool }` — ответ `/me/subscription`.
pub fn premium_response(premium: bool) -> Value {
    json!({ "premium": premium })
}
