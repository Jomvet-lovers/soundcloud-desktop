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
    ColdRefreshService, FOLLOWINGS, LIKED_PLAYLISTS, LIKED_TRACKS, OWNED_PLAYLISTS, OWNED_TRACKS,
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

    /// Cold-read /me/likes/tracks: user_likes_tracks JOIN indexed_tracks.
    /// На пустом зеркале — синхронный seed из SC через ensure_collection.
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

        let offset = page.max(0) * limit;
        let rows: Vec<(Option<sqlx::types::Json<Value>>,)> = sqlx::query_as(
            "SELECT it.raw_sc_data \
             FROM user_likes_tracks ulk \
             LEFT JOIN indexed_tracks it ON it.sc_track_id = ulk.sc_track_id \
             WHERE ulk.user_id = $1 AND ulk.wanted_state = true \
             ORDER BY ulk.ctid DESC \
             LIMIT $2 OFFSET $3",
        )
        .bind(sc_user_id)
        .bind(limit + 1)
        .bind(offset)
        .fetch_all(&self.pg)
        .await?;

        let has_more = rows.len() as i64 > limit;
        let mut collection: Vec<Value> = rows
            .into_iter()
            .take(limit as usize)
            .filter_map(|(raw,)| raw.map(|j| j.0))
            .collect();
        for t in collection.iter_mut() {
            if let Some(obj) = t.as_object_mut() {
                obj.insert("user_favorite".into(), Value::Bool(true));
            }
        }

        // Запись лайков в user_events для taste-vector — фоном, ошибка не критична.
        let events = self.events.clone();
        let user_id = sc_user_id.to_string();
        let urns: Vec<String> = collection
            .iter()
            .filter_map(|t| t.get("urn").and_then(|v| v.as_str()).map(String::from))
            .collect();
        tokio::spawn(async move {
            if let Err(e) = events.ensure_likes_recorded(&user_id, &urns).await {
                debug!(error = %e, "seedLikesTaste failed");
            }
        });

        Ok(ListPageResult {
            collection,
            page,
            page_size: limit,
            has_more,
        })
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
        Ok(read_mirror_with_json_payload(
            &self.pg,
            "user_likes_playlists",
            "playlist_urn",
            "cached_playlists",
            "playlist_urn",
            "payload",
            sc_user_id,
            true,
            page,
            limit,
        )
        .await?)
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
        Ok(read_mirror_with_json_payload(
            &self.pg,
            "user_followings",
            "target_user_urn",
            "cached_users",
            "user_urn",
            "payload",
            sc_user_id,
            true,
            page,
            limit,
        )
        .await?)
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
        Ok(read_owned_payload(&self.pg, "user_owned_playlists", sc_user_id, page, limit).await?)
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
            read_owned_payload(&self.pg, "user_owned_tracks", sc_user_id, page, limit).await?;
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }
}

async fn read_owned_payload(
    pg: &PgPool,
    mirror_table: &str,
    sc_user_id: &str,
    page: i64,
    limit: i64,
) -> AppResult<ListPageResult<Value>> {
    let offset = page.max(0) * limit;
    let sql = format!(
        "SELECT payload FROM {mirror_table} \
         WHERE user_id = $1 AND payload IS NOT NULL \
         ORDER BY created_at DESC \
         LIMIT $2 OFFSET $3"
    );
    let rows: Vec<(Option<sqlx::types::Json<Value>>,)> = sqlx::query_as(&sql)
        .bind(sc_user_id)
        .bind(limit + 1)
        .bind(offset)
        .fetch_all(pg)
        .await?;
    let has_more = rows.len() as i64 > limit;
    let collection: Vec<Value> = rows
        .into_iter()
        .take(limit as usize)
        .filter_map(|(raw,)| raw.map(|j| j.0))
        .collect();
    Ok(ListPageResult {
        collection,
        page,
        page_size: limit,
        has_more,
    })
}

/// JOIN-чтение mirror'а с json-payload из shared cache. Используется для
/// playlists/users, у которых одна колонка с целым SC-payload (в отличие от
/// треков, где payload в indexed_tracks.raw_sc_data берётся именованно).
/// Имена колонок-ключей в mirror и cache часто различаются: в followings
/// зеркало хранит `target_user_urn`, а cached_users — `user_urn`.
async fn read_mirror_with_json_payload(
    pg: &PgPool,
    mirror_table: &str,
    mirror_key_col: &str,
    cache_table: &str,
    cache_key_col: &str,
    cache_payload_col: &str,
    sc_user_id: &str,
    wanted_only: bool,
    page: i64,
    limit: i64,
) -> AppResult<ListPageResult<Value>> {
    let offset = page.max(0) * limit;
    let wanted_filter = if wanted_only {
        "AND m.wanted_state = true"
    } else {
        ""
    };
    let sql = format!(
        "SELECT c.{cache_payload_col} \
         FROM {mirror_table} m \
         LEFT JOIN {cache_table} c ON c.{cache_key_col} = m.{mirror_key_col} \
         WHERE m.user_id = $1 {wanted_filter} \
         ORDER BY m.created_at DESC \
         LIMIT $2 OFFSET $3"
    );
    let rows: Vec<(Option<sqlx::types::Json<Value>>,)> = sqlx::query_as(&sql)
        .bind(sc_user_id)
        .bind(limit + 1)
        .bind(offset)
        .fetch_all(pg)
        .await?;
    let has_more = rows.len() as i64 > limit;
    let collection: Vec<Value> = rows
        .into_iter()
        .take(limit as usize)
        .filter_map(|(raw,)| raw.map(|j| j.0))
        .collect();
    Ok(ListPageResult {
        collection,
        page,
        page_size: limit,
        has_more,
    })
}

/// `{ premium: bool }` — ответ `/me/subscription`.
pub fn premium_response(premium: bool) -> Value {
    json!({ "premium": premium })
}
