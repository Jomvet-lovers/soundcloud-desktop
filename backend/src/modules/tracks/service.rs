use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, plain_query, sc_list_page, sc_search_page, ListCacheService,
    ListPageResult, ScListPageArgs, ScSearchArgs,
};
use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::cold_refresh::ColdRefreshService;
use crate::modules::likes::cold as likes_cold;
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::{ScClient, ScReadService, SearchType};

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
    read: Arc<ScReadService>,
}

impl TracksService {
    pub fn new(
        sc: ScClient,
        pg: PgPool,
        list_cache: Arc<ListCacheService>,
        sync_queue: Arc<SyncQueueService>,
        cold_refresh: Arc<ColdRefreshService>,
        tokens: Arc<TokenProvider>,
        read: Arc<ScReadService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            pg,
            list_cache,
            sync_queue,
            cold_refresh,
            tokens,
            read,
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
        // Plain text query → apiv2 search (token-free). id-batch / genre+tag facets stay
        // on apiv1 (`/tracks?ids` and faceted search have no token-free apiv2 mapping here).
        let mut result = match plain_query(&extra) {
            Some(q) => {
                sc_search_page(ScSearchArgs {
                    list_cache: &self.list_cache,
                    read: &self.read,
                    kind: TokenKind::UserFirst(session_id),
                    ty: SearchType::Tracks,
                    q,
                    cache_key: &cache_key,
                    ttl: TTL_SEARCH,
                    page,
                    limit,
                })
                .await?
            }
            None => {
                sc_list_page(ScListPageArgs {
                    list_cache: &self.list_cache,
                    sc: &self.sc,
                    tokens: &self.tokens,
                    read: &self.read,
                    kind: TokenKind::UserFirst(session_id),
                    cache_key: &cache_key,
                    ttl: TTL_SEARCH,
                    scope: CacheScope::Shared,
                    session_id: None,
                    page,
                    limit,
                    path: "/tracks".into(),
                    extra_params: extra,
                    apiv2: false, // id-batch / faceted search → apiv1
                })
                .await?
            }
        };
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
            let row: Option<crate::modules::tracks::TrackRow> = sqlx::query_file_as!(
                crate::modules::tracks::TrackRow,
                "queries/tracks/service/find_by_sc_track_id.sql",
                &sc_track_id
            )
            .fetch_optional(&self.pg)
            .await?;
            if let Some(track_row) = row {
                // Sharing-guard: приватные треки видит только uploader. Owner
                // зайдёт сюда же — мы не отдаём `/me/track-by-id` отдельным
                // эндпоинтом, /tracks/{urn} один на всех.
                if track_row.sharing != "public" {
                    // uploader_sc_user_id — голый id, sc_user_id из сессии — URN.
                    let me = crate::common::sc_ids::extract_sc_id(sc_user_id);
                    let is_owner = track_row
                        .uploader_sc_user_id
                        .as_deref()
                        .map(|u| u == me)
                        .unwrap_or(false);
                    if !is_owner {
                        return Err(crate::error::AppError::not_found("Track not found"));
                    }
                }
                let synced_at = track_row.sc_synced_at;
                let pg = self.pg.clone();
                let id = sc_track_id.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query_file!("queries/tracks/service/touch_last_read.sql", &id)
                        .execute(&pg)
                        .await;
                });
                if self.cold_refresh.is_track_stale(Some(synced_at)) {
                    let refresh = self.cold_refresh.clone();
                    let urn = track_urn.to_string();
                    tokio::spawn(async move {
                        if let Err(e) = refresh
                            .refresh_track(&urn, TokenKind::UserFirst(session_id))
                            .await
                        {
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
                let fetched = self
                    .read
                    .track_by_id(TokenKind::UserFirst(session_id), &sc_track_id)
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
        // Мутация на треке владельца — только user-token, без public-fallback
        // (SC сам отвергнет PUT на чужой трек).
        let chain = self.tokens.chain(TokenKind::User(session_id)).await?;
        let resp = try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/tracks/{track_urn}");
            let body = body.clone();
            async move { sc.api_put_value(&path, &tok, Some(&body)).await }
        })
        .await?;

        // Reconcile приватности локально: дженерик-PUT с `{track:{sharing}}` (или
        // плоским `{sharing}`) меняет SC, а наш read-фильтр (project_many_public)
        // иначе держал бы старое значение до cold-refresh → утечка приватного на
        // публичных путях. set_sharing делает это явно; здесь подстраховываемся.
        let sharing = body
            .get("track")
            .and_then(|t| t.get("sharing"))
            .or_else(|| body.get("sharing"))
            .and_then(|v| v.as_str());
        if let Some(s) = sharing.filter(|s| *s == "public" || *s == "private") {
            let _ = sqlx::query_file!(
                "queries/tracks/service/reconcile_sharing.sql",
                extract_sc_id(track_urn),
                s
            )
            .execute(&self.pg)
            .await;
        }
        Ok(resp)
    }

    /// Смена приватности своего трека. Owner-check по нашей БД, optimistic
    /// апдейт `tracks.sharing` (приват-фильтр срабатывает сразу), write-back в
    /// SC через sync_queue (ban-resilient). `sharing` ∈ {public, private}.
    pub async fn set_sharing(
        &self,
        sc_user_id: &str,
        track_urn: &str,
        sharing: &str,
    ) -> AppResult<Value> {
        if sharing != "public" && sharing != "private" {
            return Err(crate::error::AppError::bad_request(
                "sharing must be 'public' or 'private'",
            ));
        }
        let sc_track_id = extract_sc_id(track_urn).to_string();
        let me = extract_sc_id(sc_user_id);

        let uploader: Option<Option<String>> =
            sqlx::query_file_scalar!("queries/tracks/service/find_uploader.sql", &sc_track_id)
                .fetch_optional(&self.pg)
                .await?;
        // 404 (а не 403) для чужого/несуществующего — не палим факт наличия.
        match uploader {
            Some(u) if u.as_deref() == Some(me) => {}
            _ => return Err(crate::error::AppError::not_found("Track not found")),
        }

        sqlx::query_file!(
            "queries/tracks/service/reconcile_sharing.sql",
            &sc_track_id,
            sharing
        )
        .execute(&self.pg)
        .await?;
        self.sync_queue
            .enqueue(
                sc_user_id,
                "track_sharing",
                track_urn,
                Some(&json!({ "sharing": sharing })),
            )
            .await?;
        Ok(json!({ "urn": track_urn, "sharing": sharing }))
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
            read: &self.read,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &cache_key,
            ttl: TTL_COMMENTS,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{}/comments", extract_sc_id(track_urn)),
            extra_params: vec![("threaded".into(), "0".into())], // apiv2 comments require it
            apiv2: true,
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
            read: &self.read,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &format!("track-favoriters:{track_urn}"),
            ttl: TTL_FAVORITERS,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{track_urn}/favoriters"),
            extra_params: vec![],
            apiv2: false, // apiv2 has no anon favoriters endpoint (404)
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
            read: &self.read,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &format!("track-reposters:{track_urn}"),
            ttl: TTL_REPOSTERS,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{}/reposters", extract_sc_id(track_urn)),
            extra_params: vec![],
            apiv2: true,
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
            read: &self.read,
            kind: TokenKind::UserFirst(session_id),
            cache_key: &cache_key,
            ttl: TTL_RELATED,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path: format!("/tracks/{}/related", extract_sc_id(track_urn)),
            extra_params: vec![("access".into(), access.to_string())],
            apiv2: true,
        })
        .await?;
        likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut result.collection).await?;
        Ok(result)
    }
}

fn as_pairs(v: &[(String, String)]) -> Vec<(&str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
