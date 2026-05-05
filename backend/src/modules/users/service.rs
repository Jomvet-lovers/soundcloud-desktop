use std::sync::Arc;

use serde_json::Value;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, extract_sc_cursor, FetchChunkResult, GetPageOptions, ListCacheService,
    ListPageResult,
};
use crate::error::{AppError, AppResult};
use crate::modules::local_likes::LocalLikesService;
use crate::sc::ScClient;

const TTL_SEARCH: u64 = 300;
const TTL_FOLLOWS: u64 = 600;
const TTL_USER_TRACKS: u64 = 600;
const TTL_USER_PLAYLISTS: u64 = 600;
const TTL_USER_LIKES: u64 = 600;

pub struct UsersService {
    sc: ScClient,
    list_cache: Arc<ListCacheService>,
    local_likes: Arc<LocalLikesService>,
}

impl UsersService {
    pub fn new(
        sc: ScClient,
        list_cache: Arc<ListCacheService>,
        local_likes: Arc<LocalLikesService>,
    ) -> Arc<Self> {
        Arc::new(Self { sc, list_cache, local_likes })
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
        let liked = self.local_likes.get_liked_track_ids(sc_user_id, &urns).await?;
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

    async fn list_page(
        &self,
        cache_key: &str,
        ttl: u64,
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
                    scope: CacheScope::Shared,
                    session_id: None,
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
                        let items = resp
                            .get("collection")
                            .and_then(|v| v.as_array().cloned())
                            .unwrap_or_default();
                        let next_cursor = resp
                            .get("next_href")
                            .and_then(|v| v.as_str())
                            .and_then(|h| extract_sc_cursor(Some(h)));
                        Ok::<_, AppError>(FetchChunkResult { items, next_cursor })
                    }
                },
            )
            .await
    }

    pub async fn search(
        &self,
        token: &str,
        page: i64,
        limit: i64,
        q: Option<String>,
        ids: Option<String>,
    ) -> AppResult<ListPageResult<Value>> {
        let mut extra: Vec<(String, String)> = Vec::new();
        if let Some(v) = q.clone() { extra.push(("q".into(), v)); }
        if let Some(v) = ids.clone() { extra.push(("ids".into(), v)); }
        let key = build_list_cache_key("users-search", &as_pairs(&extra));
        self.list_page(&key, TTL_SEARCH, page, limit, "/users".into(), token.to_string(), extra)
            .await
    }

    pub async fn get_by_id(&self, token: &str, user_urn: &str) -> AppResult<Value> {
        self.sc.api_get_value(&format!("/users/{user_urn}"), token, None).await
    }

    pub async fn get_followers(
        &self,
        token: &str,
        user_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            &format!("user-followers:{user_urn}"),
            TTL_FOLLOWS,
            page,
            limit,
            format!("/users/{user_urn}/followers"),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_followings(
        &self,
        token: &str,
        user_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            &format!("user-followings:{user_urn}"),
            TTL_FOLLOWS,
            page,
            limit,
            format!("/users/{user_urn}/followings"),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_is_following(
        &self,
        token: &str,
        user_urn: &str,
        following_urn: &str,
    ) -> AppResult<bool> {
        match self
            .sc
            .api_get_value(
                &format!("/users/{user_urn}/followings/{following_urn}"),
                token,
                None,
            )
            .await
        {
            Ok(v) => Ok(v.get("urn").and_then(|x| x.as_str()) == Some(following_urn)),
            Err(_) => Ok(false),
        }
    }

    pub async fn get_tracks(
        &self,
        token: &str,
        sc_user_id: &str,
        user_urn: &str,
        page: i64,
        limit: i64,
        access: &str,
    ) -> AppResult<ListPageResult<Value>> {
        let key = build_list_cache_key(
            &format!("user-tracks:{user_urn}"),
            &[("access", access.to_string())],
        );
        let mut result = self
            .list_page(
                &key,
                TTL_USER_TRACKS,
                page,
                limit,
                format!("/users/{user_urn}/tracks"),
                token.to_string(),
                vec![("access".into(), access.to_string())],
            )
            .await?;
        self.apply_local_like_flags(sc_user_id, &mut result.collection).await?;
        Ok(result)
    }

    pub async fn get_playlists(
        &self,
        token: &str,
        user_urn: &str,
        page: i64,
        limit: i64,
        access: &str,
        show_tracks: Option<String>,
    ) -> AppResult<ListPageResult<Value>> {
        let mut extra: Vec<(String, String)> = vec![("access".into(), access.to_string())];
        if let Some(v) = show_tracks { extra.push(("show_tracks".into(), v)); }
        let key = build_list_cache_key(
            &format!("user-playlists:{user_urn}"),
            &as_pairs(&extra),
        );
        self.list_page(
            &key,
            TTL_USER_PLAYLISTS,
            page,
            limit,
            format!("/users/{user_urn}/playlists"),
            token.to_string(),
            extra,
        )
        .await
    }

    pub async fn get_liked_tracks(
        &self,
        token: &str,
        sc_user_id: &str,
        user_urn: &str,
        page: i64,
        limit: i64,
        access: &str,
    ) -> AppResult<ListPageResult<Value>> {
        let key = build_list_cache_key(
            &format!("user-liked-tracks:{user_urn}"),
            &[("access", access.to_string())],
        );
        let mut result = self
            .list_page(
                &key,
                TTL_USER_LIKES,
                page,
                limit,
                format!("/users/{user_urn}/likes/tracks"),
                token.to_string(),
                vec![("access".into(), access.to_string())],
            )
            .await?;
        self.apply_local_like_flags(sc_user_id, &mut result.collection).await?;
        Ok(result)
    }

    pub async fn get_liked_playlists(
        &self,
        token: &str,
        user_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            &format!("user-liked-playlists:{user_urn}"),
            TTL_USER_LIKES,
            page,
            limit,
            format!("/users/{user_urn}/likes/playlists"),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_web_profiles(&self, token: &str, user_urn: &str) -> AppResult<Value> {
        self.sc
            .api_get_value(&format!("/users/{user_urn}/web-profiles"), token, None)
            .await
    }
}

fn as_pairs<'a>(v: &'a [(String, String)]) -> Vec<(&'a str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
