use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, extract_sc_cursor, FetchChunkResult, GetPageOptions, ListCacheService,
    ListPageResult,
};
use crate::error::{AppError, AppResult};
use crate::modules::cold_refresh::ColdRefreshService;
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::{self, ScClient};

const TTL_SEARCH: u64 = 300;
const TTL_TRACKS: u64 = 1800;
const TTL_REPOSTERS: u64 = 600;

pub struct PlaylistsService {
    sc: ScClient,
    pg: PgPool,
    list_cache: Arc<ListCacheService>,
    sync_queue: Arc<SyncQueueService>,
    cold_refresh: Arc<ColdRefreshService>,
}

impl PlaylistsService {
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

    /// Создание плейлиста — без cold: фронту нужен URN сразу. Идём в SC, на
    /// ban-ответ кладём в sync_queue с nonce-URN (несколько параллельных
    /// create'ов одного юзера не должны дедупиться друг с другом). cached_*
    /// заполнит refresh_owned_playlists по следующему чтению /me/playlists.
    pub async fn create(&self, token: &str, sc_user_id: &str, body: &Value) -> AppResult<Value> {
        match self
            .sc
            .api_post_value("/playlists", token, Some(body))
            .await
        {
            Ok(v) => Ok(v),
            Err(e) if sc::is_ban_error(&e) => {
                let nonce = format!("new:{}", Uuid::new_v4());
                self.sync_queue
                    .enqueue(sc_user_id, "playlist_create", &nonce, Some(body))
                    .await?;
                Ok(json!({
                    "status": "queued",
                    "actionType": "playlist_create",
                    "targetUrn": nonce,
                }))
            }
            Err(e) => Err(e),
        }
    }

    /// Cold-read /playlists/{urn}: cached_playlists → miss → SC + upsert.
    /// Query-params (access/show_tracks/secret_token) учитываются только при
    /// miss — кеш на URN-уровне общий. secret_token-запросы идут мимо кеша.
    pub async fn get_by_id(
        &self,
        token: &str,
        playlist_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        let has_secret = params.iter().any(|(k, _)| k == "secret_token");
        if has_secret {
            return self
                .sc
                .api_get_value(&format!("/playlists/{playlist_urn}"), token, Some(params))
                .await;
        }
        let cached: Option<(
            sqlx::types::Json<Value>,
            Option<chrono::DateTime<chrono::Utc>>,
        )> = sqlx::query_as(
            "SELECT payload, synced_at FROM cached_playlists WHERE playlist_urn = $1",
        )
        .bind(playlist_urn)
        .fetch_optional(&self.pg)
        .await?;
        if let Some((j, synced_at)) = cached {
            let pg = self.pg.clone();
            let urn = playlist_urn.to_string();
            tokio::spawn(async move {
                let _ = sqlx::query(
                    "UPDATE cached_playlists SET last_read_at = now() \
                     WHERE playlist_urn = $1 \
                       AND (last_read_at IS NULL \
                            OR last_read_at < now() - INTERVAL '5 minutes')",
                )
                .bind(&urn)
                .execute(&pg)
                .await;
            });
            if self.cold_refresh.is_playlist_stale(synced_at) {
                let refresh = self.cold_refresh.clone();
                let urn = playlist_urn.to_string();
                let tok = token.to_string();
                tokio::spawn(async move {
                    if let Err(e) = refresh.refresh_playlist(&urn, &tok).await {
                        tracing::debug!(error = %e, urn = %urn, "playlist refresh failed");
                    }
                });
            }
            return Ok(j.0);
        }
        let fetched: Value = self
            .sc
            .api_get_value(&format!("/playlists/{playlist_urn}"), token, Some(params))
            .await?;
        sqlx::query(
            "INSERT INTO cached_playlists (playlist_urn, payload, synced_at, last_read_at) \
             VALUES ($1, $2, now(), now()) \
             ON CONFLICT (playlist_urn) DO UPDATE SET \
                 payload = EXCLUDED.payload, synced_at = now(), last_read_at = now()",
        )
        .bind(playlist_urn)
        .bind(&fetched)
        .execute(&self.pg)
        .await?;
        Ok(fetched)
    }

    /// Оптимистичный update: только enqueue. SC-вызов и инвалидация
    /// cached_playlists произойдут в action handler'е.
    pub async fn update(
        &self,
        sc_user_id: &str,
        playlist_urn: &str,
        body: &Value,
    ) -> AppResult<Value> {
        self.sync_queue
            .enqueue(sc_user_id, "playlist_update", playlist_urn, Some(body))
            .await?;
        Ok(json!({
            "status": "queued",
            "actionType": "playlist_update",
            "targetUrn": playlist_urn,
        }))
    }

    /// Оптимистичный delete: убираем строку из user_owned_playlists (UI сразу
    /// перестаёт показывать плейлист в /me/playlists), очищаем cached_playlists
    /// и его tracks-mirror. SC delete — фоном через worker.
    pub async fn delete(&self, sc_user_id: &str, playlist_urn: &str) -> AppResult<Value> {
        let mut tx = self.pg.begin().await?;
        sqlx::query("DELETE FROM user_owned_playlists WHERE user_id = $1 AND playlist_urn = $2")
            .bind(sc_user_id)
            .bind(playlist_urn)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM cached_playlists WHERE playlist_urn = $1")
            .bind(playlist_urn)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM cached_playlist_tracks WHERE playlist_urn = $1")
            .bind(playlist_urn)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.sync_queue
            .enqueue(sc_user_id, "playlist_delete", playlist_urn, None)
            .await?;
        Ok(json!({
            "status": "queued",
            "actionType": "playlist_delete",
            "targetUrn": playlist_urn,
        }))
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
