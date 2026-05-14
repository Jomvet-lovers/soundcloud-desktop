use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, extract_sc_cursor, FetchChunkResult, GetPageOptions, ListCacheService,
    ListPageResult,
};
use crate::common::sc_ids::extract_sc_id;
use crate::error::{AppError, AppResult};
use crate::modules::cold_refresh::ColdRefreshService;
use crate::modules::likes::cold as likes_cold;
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::ScClient;

const TTL_SEARCH: u64 = 300;
const TTL_RELATED: u64 = 86400;
const TTL_COMMENTS: u64 = 600;
const TTL_FAVORITERS: u64 = 600;
const TTL_REPOSTERS: u64 = 600;

pub struct TracksService {
    sc: ScClient,
    pg: PgPool,
    list_cache: Arc<ListCacheService>,
    sync_queue: Arc<SyncQueueService>,
    cold_refresh: Arc<ColdRefreshService>,
}

impl TracksService {
    pub fn new(
        sc: ScClient,
        pg: PgPool,
        list_cache: Arc<ListCacheService>,
        sync_queue: Arc<SyncQueueService>,
        cold_refresh: Arc<ColdRefreshService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            pg,
            list_cache,
            sync_queue,
            cold_refresh,
        })
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
                        let resp: Value = sc.api_get_value(&path, &token, Some(&params)).await?;
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
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }

    /// Cold read /tracks/{urn}: сначала indexed_tracks, на miss — SC + upsert.
    /// secret_token-запросы (приватные треки) идут мимо кеша.
    pub async fn get_by_id(
        &self,
        token: &str,
        sc_user_id: &str,
        track_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        let has_secret = params.iter().any(|(k, _)| k == "secret_token");
        let sc_track_id = extract_sc_id(track_urn).to_string();

        let mut track: Value = if has_secret {
            self.sc
                .api_get_value(&format!("/tracks/{track_urn}"), token, Some(params))
                .await?
        } else {
            let cached: Option<(sqlx::types::Json<Value>, Option<chrono::DateTime<chrono::Utc>>)> =
                sqlx::query_as(
                    "SELECT raw_sc_data, synced_at FROM indexed_tracks WHERE sc_track_id = $1",
                )
                .bind(&sc_track_id)
                .fetch_optional(&self.pg)
                .await?;
            if let Some((j, synced_at)) = cached {
                let pg = self.pg.clone();
                let id = sc_track_id.clone();
                // Условный UPDATE: на горячих треках (тысячи rps на топ-100)
                // переписывать строку каждый раз — лишний нагруз. Достаточно
                // обновлять раз в 5 минут, eviction-cutoff гораздо длиннее.
                tokio::spawn(async move {
                    let _ = sqlx::query(
                        "UPDATE indexed_tracks SET last_read_at = now() \
                         WHERE sc_track_id = $1 \
                           AND (last_read_at IS NULL \
                                OR last_read_at < now() - INTERVAL '5 minutes')",
                    )
                    .bind(&id)
                    .execute(&pg)
                    .await;
                });
                if self.cold_refresh.is_track_stale(synced_at) {
                    let refresh = self.cold_refresh.clone();
                    let urn = track_urn.to_string();
                    let tok = token.to_string();
                    tokio::spawn(async move {
                        if let Err(e) = refresh.refresh_track(&urn, &tok).await {
                            tracing::debug!(error = %e, urn = %urn, "track refresh failed");
                        }
                    });
                }
                j.0
            } else {
                let fetched: Value = self
                    .sc
                    .api_get_value(&format!("/tracks/{track_urn}"), token, Some(params))
                    .await?;
                sqlx::query(
                    "INSERT INTO indexed_tracks (sc_track_id, raw_sc_data, synced_at, last_read_at) \
                     VALUES ($1, $2, now(), now()) \
                     ON CONFLICT (sc_track_id) DO UPDATE SET \
                         raw_sc_data = EXCLUDED.raw_sc_data, synced_at = now(), last_read_at = now()",
                )
                .bind(&sc_track_id)
                .bind(&fetched)
                .execute(&self.pg)
                .await?;
                fetched
            }
        };

        let mut single = vec![track];
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut single).await?;
        track = single.into_iter().next().unwrap_or(Value::Null);
        Ok(track)
    }

    pub async fn update(&self, token: &str, track_urn: &str, body: &Value) -> AppResult<Value> {
        self.sc
            .api_put_value(&format!("/tracks/{track_urn}"), token, Some(body))
            .await
    }

    pub async fn delete(&self, token: &str, track_urn: &str) -> AppResult<Value> {
        self.sc
            .api_delete(&format!("/tracks/{track_urn}"), token)
            .await
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

    /// Оптимистичный комментарий: всегда через sync_queue. Фронт не получает
    /// сам comment-payload (SC отдаст id позже после синка) — только подтверждение.
    pub async fn create_comment(
        &self,
        sc_user_id: &str,
        track_urn: &str,
        body: &Value,
    ) -> AppResult<Value> {
        self.sync_queue
            .enqueue(sc_user_id, "comment", track_urn, Some(body))
            .await?;
        Ok(json!({ "status": "queued", "actionType": "comment", "targetUrn": track_urn }))
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
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }
}

fn as_pairs<'a>(v: &'a [(String, String)]) -> Vec<(&'a str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
