use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::debug;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    extract_sc_cursor, FetchChunkResult, GetPageOptions, ListCacheService, ListPageResult,
};
use crate::error::AppResult;
use crate::modules::cold_refresh::{
    read_collection_page, ColdRefreshService, FOLLOWINGS, LIKED_PLAYLISTS, LIKED_TRACKS,
    OWNED_PLAYLISTS, OWNED_TRACKS,
};
use crate::modules::events::EventsService;
use crate::modules::likes::cold as likes_cold;
use crate::modules::sync_queue::mirror::{self, FOLLOWINGS as FOLLOWINGS_MIRROR};
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::ScClient;

const TTL_FEED: u64 = 60;
const TTL_FOLLOWINGS_TRACKS: u64 = 60;
const TTL_FOLLOWERS: u64 = 600;

pub struct MeService {
    sc: ScClient,
    pg: PgPool,
    list_cache: Arc<ListCacheService>,
    sync_queue: Arc<SyncQueueService>,
    cold_refresh: Arc<ColdRefreshService>,
    events: Arc<EventsService>,
}

impl MeService {
    pub fn new(
        sc: ScClient,
        pg: PgPool,
        list_cache: Arc<ListCacheService>,
        sync_queue: Arc<SyncQueueService>,
        cold_refresh: Arc<ColdRefreshService>,
        events: Arc<EventsService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            pg,
            list_cache,
            sync_queue,
            cold_refresh,
            events,
        })
    }

    pub async fn get_profile(&self, token: &str) -> AppResult<Value> {
        self.sc.api_get_value("/me", token, None).await
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
        likes_cold::apply_user_favorite_flag_to_activities(
            &self.pg,
            sc_user_id,
            &mut result.collection,
        )
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
        likes_cold::apply_user_favorite_flag_to_activities(
            &self.pg,
            sc_user_id,
            &mut result.collection,
        )
        .await?;
        Ok(result)
    }

    pub async fn get_liked_tracks(
        &self,
        token: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
        access: &str,
    ) -> AppResult<ListPageResult<Value>> {
        self.cold_refresh
            .ensure_collection(
                LIKED_TRACKS,
                sc_user_id,
                token,
                &[("access".into(), access.to_string())],
            )
            .await?;
        let mut result =
            read_collection_page(&self.pg, &LIKED_TRACKS, sc_user_id, page, limit).await?;
        for t in result.collection.iter_mut() {
            if let Some(obj) = t.as_object_mut() {
                obj.insert("user_favorite".into(), Value::Bool(true));
            }
        }

        let events = self.events.clone();
        let user_id = sc_user_id.to_string();
        let urns: Vec<String> = result
            .collection
            .iter()
            .filter_map(|t| t.get("urn").and_then(|v| v.as_str()).map(String::from))
            .collect();
        tokio::spawn(async move {
            if let Err(e) = events.ensure_likes_recorded(&user_id, &urns).await {
                debug!(error = %e, "seedLikesTaste failed");
            }
        });

        Ok(result)
    }

    pub async fn get_liked_playlists(
        &self,
        token: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.cold_refresh
            .ensure_collection(LIKED_PLAYLISTS, sc_user_id, token, &[])
            .await?;
        read_collection_page(&self.pg, &LIKED_PLAYLISTS, sc_user_id, page, limit).await
    }

    pub async fn get_followings(
        &self,
        token: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.cold_refresh
            .ensure_collection(FOLLOWINGS, sc_user_id, token, &[])
            .await?;
        read_collection_page(&self.pg, &FOLLOWINGS, sc_user_id, page, limit).await
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
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }

    pub async fn follow_user(&self, sc_user_id: &str, target_user_urn: &str) -> AppResult<Value> {
        mirror::set_wanted(&self.pg, FOLLOWINGS_MIRROR, sc_user_id, target_user_urn).await?;
        self.sync_queue
            .enqueue(sc_user_id, "follow_user", target_user_urn, None)
            .await?;
        Ok(json!({ "status": "queued", "actionType": "follow_user" }))
    }

    pub async fn unfollow_user(&self, sc_user_id: &str, target_user_urn: &str) -> AppResult<Value> {
        mirror::clear_wanted(&self.pg, FOLLOWINGS_MIRROR, sc_user_id, target_user_urn).await?;
        self.sync_queue
            .enqueue(sc_user_id, "unfollow_user", target_user_urn, None)
            .await?;
        Ok(json!({ "status": "queued", "actionType": "unfollow_user" }))
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

    /// Owned playlists юзера, ВКЛЮЧАЯ приватные. Payload хранится в самом
    /// user_owned_playlists.payload (а не в cached_playlists), потому что
    /// приватный subset не должен утекать через shared cache.
    pub async fn get_playlists(
        &self,
        token: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.cold_refresh
            .ensure_collection(OWNED_PLAYLISTS, sc_user_id, token, &[])
            .await?;
        read_collection_page(&self.pg, &OWNED_PLAYLISTS, sc_user_id, page, limit).await
    }

    /// Owned tracks юзера, ВКЛЮЧАЯ приватные. См. комментарий к get_playlists.
    pub async fn get_tracks(
        &self,
        token: &str,
        sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.cold_refresh
            .ensure_collection(OWNED_TRACKS, sc_user_id, token, &[])
            .await?;
        let mut result =
            read_collection_page(&self.pg, &OWNED_TRACKS, sc_user_id, page, limit).await?;
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }
}

/// `{ premium: bool }` — ответ `/me/subscription`.
pub fn premium_response(premium: bool) -> Value {
    json!({ "premium": premium })
}
