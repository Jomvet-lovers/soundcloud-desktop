use std::sync::Arc;

use serde_json::{json, Value};

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, extract_sc_cursor, FetchChunkResult, GetPageOptions, ListCacheService,
    ListPageResult,
};
use crate::error::{AppError, AppResult};
use crate::modules::pending_actions::PendingActionsService;
use crate::sc::ScClient;

const TTL_SEARCH: u64 = 300;
const TTL_TRACKS: u64 = 1800;
const TTL_REPOSTERS: u64 = 600;

pub struct PlaylistsService {
    sc: ScClient,
    list_cache: Arc<ListCacheService>,
    pending: Arc<PendingActionsService>,
}

impl PlaylistsService {
    pub fn new(
        sc: ScClient,
        list_cache: Arc<ListCacheService>,
        pending: Arc<PendingActionsService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            list_cache,
            pending,
        })
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
        extra: Vec<(String, String)>,
    ) -> AppResult<ListPageResult<Value>> {
        let key = build_list_cache_key("playlists-search", &as_pairs(&extra));
        self.list_page(
            &key,
            TTL_SEARCH,
            page,
            limit,
            "/playlists".into(),
            token.to_string(),
            extra,
        )
        .await
    }

    pub async fn create(&self, token: &str, session_id: &str, body: &Value) -> AppResult<Value> {
        match self
            .sc
            .api_post_value("/playlists", token, Some(body))
            .await
        {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending
                    .enqueue(session_id, "playlist_create", "new", Some(body))
                    .await?;
                Ok(json!({ "queued": true, "actionType": "playlist_create" }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn get_by_id(
        &self,
        token: &str,
        playlist_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        self.sc
            .api_get_value(&format!("/playlists/{playlist_urn}"), token, Some(params))
            .await
    }

    pub async fn update(
        &self,
        token: &str,
        session_id: &str,
        playlist_urn: &str,
        body: &Value,
    ) -> AppResult<Value> {
        match self
            .sc
            .api_put_value(&format!("/playlists/{playlist_urn}"), token, Some(body))
            .await
        {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending
                    .enqueue(session_id, "playlist_update", playlist_urn, Some(body))
                    .await?;
                Ok(json!({
                    "queued": true,
                    "actionType": "playlist_update",
                    "targetUrn": playlist_urn,
                }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn delete(
        &self,
        token: &str,
        session_id: &str,
        playlist_urn: &str,
    ) -> AppResult<Value> {
        match self
            .sc
            .api_delete(&format!("/playlists/{playlist_urn}"), token)
            .await
        {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending
                    .enqueue(session_id, "playlist_delete", playlist_urn, None)
                    .await?;
                Ok(json!({
                    "queued": true,
                    "actionType": "playlist_delete",
                    "targetUrn": playlist_urn,
                }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn get_tracks(
        &self,
        token: &str,
        playlist_urn: &str,
        page: i64,
        limit: i64,
        extra: Vec<(String, String)>,
    ) -> AppResult<ListPageResult<Value>> {
        let key = build_list_cache_key(
            &format!("playlist-tracks:{playlist_urn}"),
            &as_pairs(&extra),
        );
        self.list_page(
            &key,
            TTL_TRACKS,
            page,
            limit,
            format!("/playlists/{playlist_urn}/tracks"),
            token.to_string(),
            extra,
        )
        .await
    }

    pub async fn get_reposters(
        &self,
        token: &str,
        playlist_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        self.list_page(
            &format!("playlist-reposters:{playlist_urn}"),
            TTL_REPOSTERS,
            page,
            limit,
            format!("/playlists/{playlist_urn}/reposters"),
            token.to_string(),
            vec![],
        )
        .await
    }
}

fn as_pairs<'a>(v: &'a [(String, String)]) -> Vec<(&'a str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
