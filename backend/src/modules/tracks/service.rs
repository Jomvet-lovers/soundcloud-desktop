use std::sync::Arc;

use serde_json::{json, Value};

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, extract_sc_cursor, FetchChunkResult, GetPageOptions, ListCacheService,
    ListPageResult,
};
use crate::error::{AppError, AppResult};
use crate::modules::local_likes::LocalLikesService;
use crate::modules::pending_actions::PendingActionsService;
use crate::sc::ScClient;

const TTL_SEARCH: u64 = 300;
const TTL_RELATED: u64 = 86400;
const TTL_COMMENTS: u64 = 600;
const TTL_FAVORITERS: u64 = 600;
const TTL_REPOSTERS: u64 = 600;

pub struct TracksService {
    sc: ScClient,
    list_cache: Arc<ListCacheService>,
    local_likes: Arc<LocalLikesService>,
    pending: Arc<PendingActionsService>,
}

impl TracksService {
    pub fn new(
        sc: ScClient,
        list_cache: Arc<ListCacheService>,
        local_likes: Arc<LocalLikesService>,
        pending: Arc<PendingActionsService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            list_cache,
            local_likes,
            pending,
        })
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
        scope: CacheScope,
        session_id: Option<&str>,
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
                    scope,
                    session_id,
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
                        let resp: Value =
                            sc.api_get_value(&path, &token, Some(&params)).await?;
                        let items: Vec<Value> = resp
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
        sc_user_id: &str,
        page: i64,
        limit: i64,
        extra: Vec<(String, String)>,
    ) -> AppResult<ListPageResult<Value>> {
        let cache_key = build_list_cache_key("tracks-search", &as_pairs(&extra));
        let mut result = self
            .list_page(
                &cache_key,
                TTL_SEARCH,
                CacheScope::Shared,
                None,
                page,
                limit,
                "/tracks".into(),
                token.to_string(),
                extra,
            )
            .await?;
        self.apply_local_like_flags(sc_user_id, &mut result.collection).await?;
        Ok(result)
    }

    pub async fn get_by_id(
        &self,
        token: &str,
        sc_user_id: &str,
        track_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        let mut track: Value = self
            .sc
            .api_get_value(&format!("/tracks/{track_urn}"), token, Some(params))
            .await?;
        let mut single = vec![track];
        self.apply_local_like_flags(sc_user_id, &mut single).await?;
        track = single.into_iter().next().unwrap_or(Value::Null);
        Ok(track)
    }

    pub async fn update(&self, token: &str, track_urn: &str, body: &Value) -> AppResult<Value> {
        self.sc
            .api_put_value(&format!("/tracks/{track_urn}"), token, Some(body))
            .await
    }

    pub async fn delete(&self, token: &str, track_urn: &str) -> AppResult<Value> {
        self.sc.api_delete(&format!("/tracks/{track_urn}"), token).await
    }

    pub async fn get_streams(
        &self,
        token: &str,
        track_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        self.sc
            .api_get_value(&format!("/tracks/{track_urn}/streams"), token, Some(params))
            .await
    }

    pub async fn get_comments(
        &self,
        token: &str,
        track_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let cache_key = format!("track-comments:{track_urn}");
        self.list_page(
            &cache_key,
            TTL_COMMENTS,
            CacheScope::Shared,
            None,
            page,
            limit,
            format!("/tracks/{track_urn}/comments"),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn create_comment(
        &self,
        token: &str,
        session_id: &str,
        track_urn: &str,
        body: &Value,
    ) -> AppResult<Value> {
        match self
            .sc
            .api_post_value(&format!("/tracks/{track_urn}/comments"), token, Some(body))
            .await
        {
            Ok(v) => Ok(v),
            Err(e) => {
                if PendingActionsService::is_ban_error(&e) {
                    self.pending
                        .enqueue(session_id, "comment", track_urn, Some(body))
                        .await?;
                    Ok(json!({
                        "queued": true,
                        "actionType": "comment",
                        "targetUrn": track_urn,
                    }))
                } else {
                    Err(e)
                }
            }
        }
    }

    pub async fn get_favoriters(
        &self,
        token: &str,
        track_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            &format!("track-favoriters:{track_urn}"),
            TTL_FAVORITERS,
            CacheScope::Shared,
            None,
            page,
            limit,
            format!("/tracks/{track_urn}/favoriters"),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_reposters(
        &self,
        token: &str,
        track_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            &format!("track-reposters:{track_urn}"),
            TTL_REPOSTERS,
            CacheScope::Shared,
            None,
            page,
            limit,
            format!("/tracks/{track_urn}/reposters"),
            token.to_string(),
            vec![],
        )
        .await
    }

    pub async fn get_related(
        &self,
        token: &str,
        sc_user_id: &str,
        track_urn: &str,
        page: i64,
        limit: i64,
        access: &str,
    ) -> AppResult<ListPageResult<Value>> {
        let cache_key = build_list_cache_key(
            &format!("track-related:{track_urn}"),
            &[("access", access.to_string())],
        );
        let mut result = self
            .list_page(
                &cache_key,
                TTL_RELATED,
                CacheScope::Shared,
                None,
                page,
                limit,
                format!("/tracks/{track_urn}/related"),
                token.to_string(),
                vec![("access".into(), access.to_string())],
            )
            .await?;
        self.apply_local_like_flags(sc_user_id, &mut result.collection).await?;
        Ok(result)
    }
}

fn as_pairs<'a>(v: &'a [(String, String)]) -> Vec<(&'a str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
