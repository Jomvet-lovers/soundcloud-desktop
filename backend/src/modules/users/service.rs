use std::sync::Arc;

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, sc_list_page, ListCacheService, ListPageResult, ScListPageArgs,
};
use crate::error::AppResult;
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::cold_refresh::{
    read_collection_page, ColdRefreshService, FOLLOWINGS, LIKED_PLAYLISTS, LIKED_TRACKS,
    OWNED_PLAYLISTS, OWNED_TRACKS,
};
use crate::modules::likes::cold as likes_cold;
use crate::sc::ScClient;

const TTL_SEARCH: u64 = 300;
const TTL_FOLLOWERS: u64 = 600;

pub struct UsersService {
    sc: ScClient,
    pg: PgPool,
    list_cache: Arc<ListCacheService>,
    cold_refresh: Arc<ColdRefreshService>,
    tokens: Arc<TokenProvider>,
}

impl UsersService {
    pub fn new(
        sc: ScClient,
        pg: PgPool,
        list_cache: Arc<ListCacheService>,
        cold_refresh: Arc<ColdRefreshService>,
        tokens: Arc<TokenProvider>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            pg,
            list_cache,
            cold_refresh,
            tokens,
        })
    }

    /// Какой токен использовать под коллекцию `target` юзера. Свои данные —
    /// User (видим private items). Чужие — UserFirst (личный токен лучше
    /// квотируется на юзера, fallback на app-pool).
    fn kind_for_target(
        &self,
        viewer_sc_user_id: &str,
        target_sc_user_id: &str,
        session_id: Uuid,
    ) -> TokenKind {
        if same_sc_user(viewer_sc_user_id, target_sc_user_id) {
            TokenKind::User(session_id)
        } else {
            TokenKind::UserFirst(session_id)
        }
    }

    // Internal helper — every arg ends up on ScListPageArgs verbatim. A wrapper
    // struct around 7 fields would only shuffle the same args from call site
    // into struct literal.
    #[allow(clippy::too_many_arguments)]
    fn list_args<'a>(
        &'a self,
        cache_key: &'a str,
        ttl: u64,
        page: i64,
        limit: i64,
        path: String,
        kind: TokenKind,
        extra_params: Vec<(String, String)>,
    ) -> ScListPageArgs<'a> {
        ScListPageArgs {
            list_cache: &self.list_cache,
            sc: &self.sc,
            tokens: &self.tokens,
            kind,
            cache_key,
            ttl,
            scope: CacheScope::Shared,
            session_id: None,
            page,
            limit,
            path,
            extra_params,
        }
    }

    pub async fn search(
        &self,
        session_id: Uuid,
        page: i64,
        limit: i64,
        q: Option<String>,
        ids: Option<String>,
    ) -> AppResult<ListPageResult<Value>> {
        let mut extra: Vec<(String, String)> = Vec::new();
        if let Some(v) = q {
            extra.push(("q".into(), v));
        }
        if let Some(v) = ids {
            extra.push(("ids".into(), v));
        }
        let key = build_list_cache_key("users-search", &as_pairs(&extra));
        sc_list_page(self.list_args(
            &key,
            TTL_SEARCH,
            page,
            limit,
            "/users".into(),
            TokenKind::UserFirst(session_id),
            extra,
        ))
        .await
    }

    /// Cold-read /users/{urn}: проекция из `users` → miss → SC + upsert.
    /// На stale hit спавним фоновой refresh (Redis SETNX дедупит дубликаты).
    pub async fn get_by_id(&self, session_id: Uuid, user_urn: &str) -> AppResult<Value> {
        let repo = crate::modules::users::UserRepository::new(self.pg.clone());
        if let Some(row) = repo.find_by_urn(user_urn).await? {
            let synced_at = row.sc_synced_at;
            // Inline (WHERE-guarded 5мин, обычно no-op) вместо spawn-на-запрос:
            // не плодим неограниченные таски/checkout'ы пула под нагрузкой.
            let _ = repo.touch_last_read(user_urn).await;
            if self.cold_refresh.is_user_stale(Some(synced_at)) {
                let refresh = self.cold_refresh.clone();
                let tokens = self.tokens.clone();
                let urn = user_urn.to_string();
                tokio::spawn(async move {
                    let chain = match tokens.chain(TokenKind::UserFirst(session_id)).await {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    if let Err(e) = refresh.refresh_user(&urn, &chain).await {
                        tracing::debug!(error = %e, urn = %urn, "user refresh failed");
                    }
                });
            }
            return Ok(crate::modules::users::project_to_sc_shape(&row));
        }
        let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
        let fetched: Value = try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/users/{user_urn}");
            async move { sc.api_get_value(&path, &tok, None).await }
        })
        .await?;
        repo.upsert_from_sc(&fetched).await?;
        Ok(fetched)
    }

    /// `/users/{urn}/followers` — единственный per-user list, который не
    /// храним cold: входящие подписчики нам бизнес-неинтересны (мы пишем
    /// followings, а не followers). Горячий TTL-кеш с user-first chain'ом.
    pub async fn get_followers(
        &self,
        session_id: Uuid,
        user_urn: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let key = format!("user-followers:{user_urn}");
        sc_list_page(self.list_args(
            &key,
            TTL_FOLLOWERS,
            page,
            limit,
            format!("/users/{user_urn}/followers"),
            TokenKind::UserFirst(session_id),
            vec![],
        ))
        .await
    }

    pub async fn get_is_following(
        &self,
        session_id: Uuid,
        user_urn: &str,
        following_urn: &str,
    ) -> AppResult<bool> {
        let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
        let res = try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/users/{user_urn}/followings/{following_urn}");
            async move { sc.api_get_value(&path, &tok, None).await }
        })
        .await;
        Ok(match res {
            Ok(v) => v.get("urn").and_then(|x| x.as_str()) == Some(following_urn),
            Err(_) => false,
        })
    }

    /// Cold-read OWNED_TRACKS для любого юзера (своего или чужого).
    /// `viewer_sc_user_id` — кто запрашивает (нужен для favorite-флага +
    /// выбора /me/* vs /users/{id}/* path и токена).
    pub async fn get_owned_tracks(
        &self,
        session_id: Uuid,
        viewer_sc_user_id: &str,
        target_sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let is_self = same_sc_user(viewer_sc_user_id, target_sc_user_id);
        let chain = self
            .tokens
            .chain(self.kind_for_target(viewer_sc_user_id, target_sc_user_id, session_id))
            .await?;
        self.cold_refresh
            .ensure_collection(OWNED_TRACKS, target_sc_user_id, is_self, &chain, &[])
            .await?;
        let mut result = read_collection_page(
            &self.pg,
            &OWNED_TRACKS,
            target_sc_user_id,
            page,
            limit,
            !is_self,
        )
            .await?;
        likes_cold::apply_user_favorite_flag(&self.pg, viewer_sc_user_id, &mut result.collection)
            .await?;
        Ok(result)
    }

    /// Cold-read OWNED_PLAYLISTS для любого юзера.
    pub async fn get_owned_playlists(
        &self,
        session_id: Uuid,
        viewer_sc_user_id: &str,
        target_sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let is_self = same_sc_user(viewer_sc_user_id, target_sc_user_id);
        let chain = self
            .tokens
            .chain(self.kind_for_target(viewer_sc_user_id, target_sc_user_id, session_id))
            .await?;
        self.cold_refresh
            .ensure_collection(OWNED_PLAYLISTS, target_sc_user_id, is_self, &chain, &[])
            .await?;
        let mut result = read_collection_page(
            &self.pg,
            &OWNED_PLAYLISTS,
            target_sc_user_id,
            page,
            limit,
            !is_self,
        )
            .await?;
        likes_cold::apply_user_favorite_flag_to_playlists(
            &self.pg,
            viewer_sc_user_id,
            &mut result.collection,
        )
        .await?;
        Ok(result)
    }

    /// Cold-read LIKED_TRACKS для любого юзера. Источник истины свежести
    /// лайков — `user_events` (туда пишет UI через `LikesService`); этот
    /// эндпоинт только проектирует список лайков, не сидирует events.
    pub async fn get_liked_tracks(
        self: &Arc<Self>,
        session_id: Uuid,
        viewer_sc_user_id: &str,
        target_sc_user_id: &str,
        page: i64,
        limit: i64,
        access: &str,
    ) -> AppResult<ListPageResult<Value>> {
        let is_self = same_sc_user(viewer_sc_user_id, target_sc_user_id);
        let chain = self
            .tokens
            .chain(self.kind_for_target(viewer_sc_user_id, target_sc_user_id, session_id))
            .await?;
        self.cold_refresh
            .ensure_collection(
                LIKED_TRACKS,
                target_sc_user_id,
                is_self,
                &chain,
                &[("access".into(), access.to_string())],
            )
            .await?;
        let mut result = read_collection_page(
            &self.pg,
            &LIKED_TRACKS,
            target_sc_user_id,
            page,
            limit,
            !is_self,
        )
            .await?;

        // Если смотрим свои лайки — каждый item автоматически user_favorite.
        // Если чужие — флаг показывает, лайкнул ли это ВЬЮВЕР (другой юзер).
        if is_self {
            for t in result.collection.iter_mut() {
                if let Some(obj) = t.as_object_mut() {
                    obj.insert("user_favorite".into(), Value::Bool(true));
                }
            }
        } else {
            likes_cold::apply_user_favorite_flag(
                &self.pg,
                viewer_sc_user_id,
                &mut result.collection,
            )
            .await?;
        }
        Ok(result)
    }

    /// Cold-read LIKED_PLAYLISTS для любого юзера.
    pub async fn get_liked_playlists(
        &self,
        session_id: Uuid,
        viewer_sc_user_id: &str,
        target_sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let is_self = same_sc_user(viewer_sc_user_id, target_sc_user_id);
        let chain = self
            .tokens
            .chain(self.kind_for_target(viewer_sc_user_id, target_sc_user_id, session_id))
            .await?;
        self.cold_refresh
            .ensure_collection(LIKED_PLAYLISTS, target_sc_user_id, is_self, &chain, &[])
            .await?;
        let mut result = read_collection_page(
            &self.pg,
            &LIKED_PLAYLISTS,
            target_sc_user_id,
            page,
            limit,
            !is_self,
        )
            .await?;
        likes_cold::apply_user_favorite_flag_to_playlists(
            &self.pg,
            viewer_sc_user_id,
            &mut result.collection,
        )
        .await?;
        Ok(result)
    }

    /// Cold-read FOLLOWINGS для любого юзера.
    pub async fn get_followings(
        &self,
        session_id: Uuid,
        viewer_sc_user_id: &str,
        target_sc_user_id: &str,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let is_self = same_sc_user(viewer_sc_user_id, target_sc_user_id);
        let chain = self
            .tokens
            .chain(self.kind_for_target(viewer_sc_user_id, target_sc_user_id, session_id))
            .await?;
        self.cold_refresh
            .ensure_collection(FOLLOWINGS, target_sc_user_id, is_self, &chain, &[])
            .await?;
        read_collection_page(
            &self.pg,
            &FOLLOWINGS,
            target_sc_user_id,
            page,
            limit,
            !is_self,
        )
            .await
    }

    pub async fn get_web_profiles(&self, session_id: Uuid, user_urn: &str) -> AppResult<Value> {
        let chain = self.tokens.chain(TokenKind::UserFirst(session_id)).await?;
        try_with_chain(&chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/users/{user_urn}/web-profiles");
            async move { sc.api_get_value(&path, &tok, None).await }
        })
        .await
    }
}

fn as_pairs(v: &[(String, String)]) -> Vec<(&str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}

/// «Свой» ли это профиль. Нормализуем оба id, т.к. viewer приходит URN'ом
/// (`soundcloud:users:NNN`), а target — голым (handler делает `extract_sc_id`).
/// Без нормализации `is_self` всегда false → `/users/{self}/*` прятал бы свои
/// приватные треки/плейлисты от самого владельца.
fn same_sc_user(viewer: &str, target: &str) -> bool {
    crate::common::sc_ids::extract_sc_id(viewer) == crate::common::sc_ids::extract_sc_id(target)
}
