use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, sc_list_page, ListCacheService, ListPageResult, ScListPageArgs,
};
use crate::error::{AppError, AppResult};
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::cold_refresh::ColdRefreshService;
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::{self, ScClient};

const TTL_SEARCH: u64 = 300;
const TTL_REPOSTERS: u64 = 600;

pub struct PlaylistsService {
    sc: ScClient,
    pg: PgPool,
    list_cache: Arc<ListCacheService>,
    sync_queue: Arc<SyncQueueService>,
    cold_refresh: Arc<ColdRefreshService>,
    tokens: Arc<TokenProvider>,
}

impl PlaylistsService {
    pub fn new(
        sc: ScClient,
        pg: PgPool,
        list_cache: Arc<ListCacheService>,
        sync_queue: Arc<SyncQueueService>,
        cold_refresh: Arc<ColdRefreshService>,
        tokens: Arc<TokenProvider>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            pg,
            list_cache,
            sync_queue,
            cold_refresh,
            tokens,
        })
    }

    pub async fn search(
        &self,
        session_id: Uuid,
        page: i64,
        limit: i64,
        extra: Vec<(String, String)>,
    ) -> AppResult<ListPageResult<Value>> {
        let key = build_list_cache_key("playlists-search", &as_pairs(&extra));
        sc_list_page(ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &key,
            ttl: TTL_SEARCH,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: "/playlists".into(),
            extra_params: extra,
        })
        .await
    }

    /// Создание плейлиста — без cold: фронту нужен URN сразу. Идём в SC, на
    /// ban-ответ кладём в sync_queue с nonce-URN (несколько параллельных
    /// create'ов одного юзера не должны дедупиться друг с другом). cached_*
    /// заполнит refresh_owned_playlists по следующему чтению /me/playlists.
    pub async fn create(
        &self,
        session_id: Uuid,
        sc_user_id: &str,
        body: &Value,
    ) -> AppResult<Value> {
        let chain = self.tokens.chain(TokenKind::User(session_id)).await?;
        let res = try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let body = body.clone();
            async move { sc.api_post_value("/playlists", &tok, Some(&body)).await }
        })
        .await;

        match res {
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

    /// Cold-read /playlists/{urn}: проекция из `playlists` → miss → SC + upsert.
    /// secret_token-запросы идут мимо кеша.
    pub async fn get_by_id(
        &self,
        session_id: Uuid,
        sc_user_id: &str,
        playlist_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        let has_secret = params.iter().any(|(k, _)| k == "secret_token");
        if has_secret {
            let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
            return try_with_chain(&chain, |tok| {
                let sc = self.sc.clone();
                let path = format!("/playlists/{playlist_urn}");
                let params = params.to_vec();
                async move { sc.api_get_value(&path, &tok, Some(&params)).await }
            })
            .await;
        }
        let repo = crate::modules::playlists::PlaylistRepository::new(self.pg.clone());
        if let Some(row) = repo.find_by_urn(playlist_urn).await? {
            // Sharing-guard для приватных плейлистов.
            if row.sharing != "public" {
                let is_owner = row
                    .owner_sc_user_id
                    .as_deref()
                    .map(|u| u == sc_user_id)
                    .unwrap_or(false);
                if !is_owner {
                    return Err(AppError::not_found("Playlist not found"));
                }
            }
            let synced_at = row.sc_synced_at;
            {
                let repo2 = crate::modules::playlists::PlaylistRepository::new(self.pg.clone());
                let urn = playlist_urn.to_string();
                tokio::spawn(async move {
                    let _ = repo2.touch_last_read(&urn).await;
                });
            }
            if self.cold_refresh.is_playlist_stale(Some(synced_at)) {
                let refresh = self.cold_refresh.clone();
                let tokens = self.tokens.clone();
                let urn = playlist_urn.to_string();
                tokio::spawn(async move {
                    let chain = match tokens.chain(TokenKind::UserFirst(session_id)).await {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    if let Err(e) = refresh.refresh_playlist(&urn, &chain).await {
                        tracing::debug!(error = %e, urn = %urn, "playlist refresh failed");
                    }
                });
            }
            return Ok(crate::modules::playlists::project_to_sc_shape(&row, None));
        }
        let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
        let fetched: Value = try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/playlists/{playlist_urn}");
            let params = params.to_vec();
            async move { sc.api_get_value(&path, &tok, Some(&params)).await }
        })
        .await?;
        repo.upsert_from_sc(&fetched).await?;
        Ok(fetched)
    }

    /// Оптимистичный update: только enqueue. SC-вызов и инвалидация
    /// нормализованной `playlists`-строки произойдут в action handler'е.
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
    /// перестаёт показывать плейлист в /me/playlists), сносим playlists +
    /// playlist_tracks. SC delete — фоном через worker.
    pub async fn delete(&self, sc_user_id: &str, playlist_urn: &str) -> AppResult<Value> {
        let mut tx = self.pg.begin().await?;
        sqlx::query("DELETE FROM user_owned_playlists WHERE user_id = $1 AND playlist_urn = $2")
            .bind(sc_user_id)
            .bind(playlist_urn)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM playlists WHERE urn = $1")
            .bind(playlist_urn)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM playlist_tracks WHERE playlist_urn = $1")
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

    /// Cold-read /playlists/{urn}/tracks: проекция из `playlist_tracks` ∪ `tracks`.
    /// На пустую строку или stale `tracks_synced_at` — синхронный seed (нужно
    /// что-то отдать клиенту), либо фоновой refresh (стандартный SWR). Это лечит
    /// 200-track loop старой ListCacheService-схемы: следуем `next_href` целиком
    /// в `refresh_playlist_tracks` и атомарно подменяем replay-storage.
    pub async fn get_tracks(
        &self,
        session_id: Uuid,
        sc_user_id: &str,
        playlist_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let repo = crate::modules::playlists::PlaylistRepository::new(self.pg.clone());

        let playlist_row = repo.find_by_urn(playlist_urn).await?;
        if let Some(row) = &playlist_row {
            if row.sharing != "public" {
                let is_owner = row
                    .owner_sc_user_id
                    .as_deref()
                    .map(|u| u == sc_user_id)
                    .unwrap_or(false);
                if !is_owner {
                    return Err(AppError::not_found("Playlist not found"));
                }
            }
        }
        let needs_seed = match &playlist_row {
            None => true,
            Some(r) => r.tracks_synced_at.is_none(),
        };

        if needs_seed {
            let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
            // Если плейлиста ещё нет в `playlists` — UPSERT meta перед track-list.
            if playlist_row.is_none() {
                let fetched: Value = try_with_chain(&chain, |tok| {
                    let sc = self.sc.clone();
                    let path = format!("/playlists/{playlist_urn}");
                    async move { sc.api_get_value(&path, &tok, None).await }
                })
                .await?;
                repo.upsert_from_sc(&fetched).await?;
            }
            self.cold_refresh
                .refresh_playlist_tracks(playlist_urn, &chain)
                .await?;
        } else if let Some(row) = &playlist_row {
            if self.cold_refresh.is_playlist_stale(row.tracks_synced_at) {
                let refresh = self.cold_refresh.clone();
                let tokens = self.tokens.clone();
                let urn = playlist_urn.to_string();
                tokio::spawn(async move {
                    let chain = match tokens.chain(TokenKind::UserFirst(session_id)).await {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    if let Err(e) = refresh.refresh_playlist_tracks(&urn, &chain).await {
                        tracing::debug!(error = %e, urn = %urn, "playlist tracks refresh failed");
                    }
                });
            }
        }

        let offset = page.max(0) * limit;
        let ids = repo.page_track_ids(playlist_urn, offset, limit + 1).await?;
        let has_more = ids.len() as i64 > limit;
        let page_ids: Vec<String> = ids.into_iter().take(limit as usize).collect();
        let collection: Vec<Value> = crate::modules::tracks::project_many(&self.pg, &page_ids)
            .await?
            .into_iter()
            .flatten()
            .collect();
        Ok(ListPageResult {
            collection,
            page,
            page_size: limit,
            has_more,
        })
    }

    pub async fn get_reposters(
        &self,
        session_id: Uuid,
        playlist_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        sc_list_page(ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &format!("playlist-reposters:{playlist_urn}"),
            ttl: TTL_REPOSTERS,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/playlists/{playlist_urn}/reposters"),
            extra_params: vec![],
        })
        .await
    }
}

fn as_pairs<'a>(v: &'a [(String, String)]) -> Vec<(&'a str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
