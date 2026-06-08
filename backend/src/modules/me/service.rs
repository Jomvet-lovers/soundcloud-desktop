use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::debug;

use crate::cache::cache_service::CacheScope;
use crate::cache::{FetchChunkResult, GetPageOptions, ListCacheService, ListPageResult};
use crate::error::AppResult;
use crate::modules::likes::cold as likes_cold;
use crate::modules::sync_queue::mirror::{self, FOLLOWINGS as FOLLOWINGS_MIRROR};
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::ScClient;

const TTL_FOLLOWINGS_TRACKS: u64 = 60;
const TTL_FOLLOWERS: u64 = 600;
const PROFILE_TTL_SEC: i64 = 600;
const PROFILE_SEED_TIMEOUT_SEC: u64 = 3;

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

    /// DB-backed profile for Library: serve the mirror immediately, revalidate
    /// from SC in the background when stale; synchronous seed on first read.
    pub async fn get_profile_cold(self: &Arc<Self>, sc_user_id: &str, token: &str) -> AppResult<Value> {
        // user_profiles ключ — URN (пишет login); ctx.sc_user_id теперь bare →
        // матчим оба варианта, LIMIT 1 по свежести (дубль URN+bare до бэкфилла).
        let row: Option<(Value, DateTime<Utc>)> = sqlx::query_as(
            "SELECT profile_json, synced_at FROM user_profiles WHERE soundcloud_user_id = ANY($1) \
             ORDER BY synced_at DESC LIMIT 1",
        )
            .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
            .fetch_one(&self.pg)
            .await
            .ok();

        if let Some((profile, synced_at)) = row {
            if Utc::now() - synced_at > Duration::seconds(PROFILE_TTL_SEC) {
                let me = Arc::clone(self);
                let uid = sc_user_id.to_string();
                let tok = token.to_string();
                tokio::spawn(async move {
                    if let Err(e) = me.refresh_profile(&uid, &tok).await {
                        debug!(error = %e, "me cold background refresh failed");
                    }
                });
            }
            return Ok(profile);
        }

        // Empty mirror: seed from SC under a short budget; if SC is slow or
        // unreachable, serve a session/users-derived stub now and finish seeding
        // in the background so Library/auth boot without blocking.
        let seed = self.refresh_profile(sc_user_id, token);
        match tokio::time::timeout(std::time::Duration::from_secs(PROFILE_SEED_TIMEOUT_SEC), seed).await {
            Ok(Ok(profile)) => Ok(profile),
            other => {
                if let Ok(Err(e)) = other {
                    debug!(error = %e, "me cold seed failed, serving stub");
                }
                let me = Arc::clone(self);
                let uid = sc_user_id.to_string();
                let tok = token.to_string();
                tokio::spawn(async move {
                    let _ = me.refresh_profile(&uid, &tok).await;
                });
                Ok(self.session_profile_stub(sc_user_id).await)
            }
        }
    }

    async fn session_profile_stub(&self, sc_user_id: &str) -> Value {
        let id: i64 = sc_user_id
            .rsplit(':')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Prefer the users mirror (carries avatar/counts); fall back to the
        // session row (username only) when the user was never synced.
        let mirrored: Option<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object( \
                 'id', $2::int8, 'urn', urn, 'username', username, 'full_name', full_name, \
                 'avatar_url', COALESCE(avatar_url, ''), 'permalink_url', COALESCE(permalink_url, ''), \
                 'followers_count', COALESCE(followers_count, 0), \
                 'followings_count', COALESCE(followings_count, 0), \
                 'track_count', COALESCE(tracks_count, 0), \
                 'playlist_count', COALESCE(playlists_count, 0), \
                 'public_favorites_count', 0) \
             FROM users WHERE sc_user_id = $1",
        )
            .bind(sc_user_id)
            .bind(id)
            .fetch_optional(&self.pg)
            .await
            .ok()
            .flatten();
        if let Some(profile) = mirrored {
            return profile;
        }

        let username: Option<String> = sqlx::query_scalar(
            "SELECT username FROM sessions WHERE soundcloud_user_id = ANY($1) AND username IS NOT NULL \
             ORDER BY updated_at DESC LIMIT 1",
        )
            .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
            .fetch_optional(&self.pg)
            .await
            .ok()
            .flatten();
        json!({
            "id": id,
            "urn": sc_user_id,
            "username": username.unwrap_or_default(),
            "avatar_url": "",
            "permalink_url": "",
            "followers_count": 0,
            "followings_count": 0,
            "track_count": 0,
            "playlist_count": 0,
            "public_favorites_count": 0,
        })
    }

    async fn refresh_profile(&self, sc_user_id: &str, token: &str) -> AppResult<Value> {
        let profile = self.sc.api_get_value("/me", token, None).await?;
        sqlx::query(
            "INSERT INTO user_profiles (soundcloud_user_id, profile_json, synced_at)
             VALUES ($1, $2, now())
             ON CONFLICT (soundcloud_user_id)
             DO UPDATE SET profile_json = EXCLUDED.profile_json, synced_at = now()",
        )
            .bind(sc_user_id)
            .bind(&profile)
            .execute(&self.pg)
            .await?;
        Ok(profile)
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
