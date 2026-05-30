use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::cache::cache_service::CacheScope;
use crate::cache::{FetchChunkResult, GetPageOptions, ListCacheService, ListPageResult};
use crate::error::AppResult;
use crate::modules::likes::cold as likes_cold;
use crate::modules::sync_queue::mirror::{self, FOLLOWINGS as FOLLOWINGS_MIRROR};
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::ScClient;

const TTL_FOLLOWINGS_TRACKS: u64 = 60;
const TTL_FOLLOWERS: u64 = 600;

/// MeService держит только то, что у нас **нет** как отдельной коллекции:
/// SC-фид (`/me/followings/tracks`),
/// followers (входящие подписчики бизнесу не нужны cold), follow/unfollow
/// мутации и `/me` профиль. Tracks/playlists/likes/followings того же юзера
/// ходят через [`UsersService`] с `target == ctx.sc_user_id` — общие mirror
/// таблицы (`user_owned_*`, `user_likes_*`, `user_followings`).
pub struct MeService {
    sc: ScClient,
    pg: PgPool,
    list_cache: Arc<ListCacheService>,
    sync_queue: Arc<SyncQueueService>,
}

impl MeService {
    pub fn new(
        sc: ScClient,
        pg: PgPool,
        list_cache: Arc<ListCacheService>,
        sync_queue: Arc<SyncQueueService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            pg,
            list_cache,
            sync_queue,
        })
    }

    pub async fn get_profile(&self, token: &str) -> AppResult<Value> {
        self.sc.api_get_value("/me", token, None).await
    }

    // Internal helper — all 9 params used to build a single ListCache GetPageOptions
    // call. Bundling them into a struct here would just add an extra layer of
    // indirection for no clarity gain.
    #[allow(clippy::too_many_arguments)]
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
                |next_href, chunk_size| {
                    let sc = sc.clone();
                    let path = path.clone();
                    let token = token.clone();
                    let extra = extra_params.clone();
                    async move {
                        let resp: Value = match next_href {
                            Some(href) => sc.api_get_absolute_value(&href, &token).await?,
                            None => {
                                let mut params: Vec<(String, String)> = extra;
                                params.push(("limit".into(), chunk_size.to_string()));
                                params.push(("linked_partitioning".into(), "true".into()));
                                sc.api_get_value(&path, &token, Some(&params)).await?
                            }
                        };
                        let items: Vec<Value> = resp
                            .get("collection")
                            .and_then(|v| v.as_array().cloned())
                            .unwrap_or_default();
                        let next_href = resp
                            .get("next_href")
                            .and_then(|v| v.as_str())
                            .map(String::from)
                            .filter(|s| !s.is_empty());
                        Ok::<_, crate::error::AppError>(FetchChunkResult { items, next_href })
                    }
                },
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
}

/// `{ premium: bool }` — ответ `/me/subscription`.
pub fn premium_response(premium: bool) -> Value {
    json!({ "premium": premium })
}
