use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, sc_list_page, ListCacheService, ListPageResult, ScListPageArgs,
};
use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
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
    tokens: Arc<TokenProvider>,
}

impl TracksService {
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

    /// Поиск треков. Сначала через user-token (точные результаты), потом —
    /// через весь app-pool в перемешанном порядке. Без рандомных юзер-сессий.
    pub async fn search(
        &self,
        session_id: uuid::Uuid,
        sc_user_id: &str,
        page: i64,
        limit: i64,
        extra: Vec<(String, String)>,
    ) -> AppResult<ListPageResult<Value>> {
        let cache_key = build_list_cache_key("tracks-search", &as_pairs(&extra));
        let mut result = sc_list_page(ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &cache_key,
            ttl: TTL_SEARCH,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: "/tracks".into(),
            extra_params: extra,
        })
        .await?;
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }

    /// Cold read /tracks/{urn}: сначала `tracks`, на miss — SC + ingest.
    /// secret_token-запросы (приватные треки) идут мимо кеша.
    pub async fn get_by_id(
        &self,
        session_id: uuid::Uuid,
        sc_user_id: &str,
        track_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        let has_secret = params.iter().any(|(k, _)| k == "secret_token");
        let sc_track_id = extract_sc_id(track_urn).to_string();

        let mut track: Value = if has_secret {
            let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
            try_with_chain(&chain, |tok| {
                let sc = self.sc.clone();
                let path = format!("/tracks/{track_urn}");
                let params = params.to_vec();
                async move { sc.api_get_value(&path, &tok, Some(&params)).await }
            })
            .await?
        } else {
            let row: Option<crate::modules::tracks::TrackRow> =
                sqlx::query_as("SELECT * FROM tracks WHERE sc_track_id = $1")
                    .bind(&sc_track_id)
                    .fetch_optional(&self.pg)
                    .await?;
            if let Some(track_row) = row {
                // Sharing-guard: приватные треки видит только uploader. Owner
                // зайдёт сюда же — мы не отдаём `/me/track-by-id` отдельным
                // эндпоинтом, /tracks/{urn} один на всех.
                if track_row.sharing != "public" {
                    let is_owner = track_row
                        .uploader_sc_user_id
                        .as_deref()
                        .map(|u| u == sc_user_id)
                        .unwrap_or(false);
                    if !is_owner {
                        return Err(crate::error::AppError::not_found("Track not found"));
                    }
                }
                let synced_at = track_row.sc_synced_at;
                let pg = self.pg.clone();
                let id = sc_track_id.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query(
                        "UPDATE tracks SET last_read_at = now() \
                         WHERE sc_track_id = $1 \
                           AND (last_read_at IS NULL \
                                OR last_read_at < now() - INTERVAL '5 minutes')",
                    )
                    .bind(&id)
                    .execute(&pg)
                    .await;
                });
                if self.cold_refresh.is_track_stale(Some(synced_at)) {
                    let refresh = self.cold_refresh.clone();
                    let tokens = self.tokens.clone();
                    let urn = track_urn.to_string();
                    tokio::spawn(async move {
                        let chain = match tokens.chain(TokenKind::UserFirst(session_id)).await {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        if let Err(e) = refresh.refresh_track(&urn, &chain).await {
                            tracing::debug!(error = %e, urn = %urn, "track refresh failed");
                        }
                    });
                }
                let projected =
                    crate::modules::tracks::project_many(&self.pg, &[sc_track_id.to_string()])
                        .await?;
                projected.into_iter().flatten().next().unwrap_or_else(|| {
                    crate::modules::tracks::project_to_sc_shape(&track_row, None)
                })
            } else {
                let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
                let fetched: Value = try_with_chain(&chain, |tok| {
                    let sc = self.sc.clone();
                    let path = format!("/tracks/{track_urn}");
                    let params = params.to_vec();
                    async move { sc.api_get_value(&path, &tok, Some(&params)).await }
                })
                .await?;
                if let Some(refresh_indexing) = self.cold_refresh.indexing_for_ingest() {
                    refresh_indexing
                        .ingest_track_from_sc(
                            &fetched,
                            crate::modules::tracks::TrackPriority::Discovery,
                        )
                        .await?;
                }
                fetched
            }
        };

        let mut single = vec![track];
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut single).await?;
        track = single.into_iter().next().unwrap_or(Value::Null);
        Ok(track)
    }

    pub async fn update(
        &self,
        session_id: uuid::Uuid,
        track_urn: &str,
        body: &Value,
    ) -> AppResult<Value> {
        // Мутация на треке владельца — только user-token, без public-fallback.
        let chain = self.tokens.chain(TokenKind::User(session_id)).await?;
        try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/tracks/{track_urn}");
            let body = body.clone();
            async move { sc.api_put_value(&path, &tok, Some(&body)).await }
        })
        .await
    }

    pub async fn delete(&self, session_id: uuid::Uuid, track_urn: &str) -> AppResult<Value> {
        let chain = self.tokens.chain(TokenKind::User(session_id)).await?;
        try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/tracks/{track_urn}");
            async move { sc.api_delete(&path, &tok).await }
        })
        .await
    }

    pub async fn get_streams(
        &self,
        session_id: uuid::Uuid,
        track_urn: &str,
        params: &[(String, String)],
    ) -> AppResult<Value> {
        let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
        try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/tracks/{track_urn}/streams");
            let params = params.to_vec();
            async move { sc.api_get_value(&path, &tok, Some(&params)).await }
        })
        .await
    }

    pub async fn get_comments(
        &self,
        session_id: uuid::Uuid,
        track_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let cache_key = format!("track-comments:{track_urn}");
        sc_list_page(ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &cache_key,
            ttl: TTL_COMMENTS,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{track_urn}/comments"),
            extra_params: vec![],
        })
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
        session_id: uuid::Uuid,
        track_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        sc_list_page(ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &format!("track-favoriters:{track_urn}"),
            ttl: TTL_FAVORITERS,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{track_urn}/favoriters"),
            extra_params: vec![],
        })
        .await
    }

    pub async fn get_reposters(
        &self,
        session_id: uuid::Uuid,
        track_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        sc_list_page(ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &format!("track-reposters:{track_urn}"),
            ttl: TTL_REPOSTERS,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{track_urn}/reposters"),
            extra_params: vec![],
        })
        .await
    }

    pub async fn get_related(
        &self,
        session_id: uuid::Uuid,
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
        let mut result = sc_list_page(ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &cache_key,
            ttl: TTL_RELATED,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{track_urn}/related"),
            extra_params: vec![("access".into(), access.to_string())],
        })
        .await?;
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }
}

fn as_pairs<'a>(v: &'a [(String, String)]) -> Vec<(&'a str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
