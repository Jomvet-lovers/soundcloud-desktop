use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::cache::cache_service::CacheScope;
use crate::cache::{
    build_list_cache_key, plain_query, sc_list_page, sc_search_page, ListCacheService,
    ListPageResult, ScListPageArgs, ScSearchArgs,
};
use crate::common::sc_ids::extract_sc_id;
use crate::error::{AppError, AppResult};
use crate::modules::auth::{try_with_chain, TokenKind, TokenProvider};
use crate::modules::cold_refresh::ColdRefreshService;
use crate::modules::playlists::PlaylistRepository;
use crate::modules::sync_queue::SyncQueueService;
use crate::sc::{self, ScClient, ScReadService, SearchType};

const TTL_SEARCH: u64 = 300;
const TTL_REPOSTERS: u64 = 600;

/// Одна правка membership плейлиста. Дельты считаются на сервере против
/// сохранённого desired-state, поэтому устаревшая клиентская вью не может
/// уронить треки (в отличие от слепого full-list replace).
pub enum TrackEdit {
    Add { track_urn: String },
    Remove { track_urn: String },
    Move { track_urn: String, to_index: i64 },
    SetOrder { track_urns: Vec<String> },
}

pub struct PlaylistsService {
    sc: ScClient,
    pg: PgPool,
    list_cache: Arc<ListCacheService>,
    sync_queue: Arc<SyncQueueService>,
    cold_refresh: Arc<ColdRefreshService>,
    tokens: Arc<TokenProvider>,
    read: Arc<ScReadService>,
}

impl PlaylistsService {
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

    pub async fn search(
        &self,
        session_id: Uuid,
        page: i64,
        limit: i64,
        extra: Vec<(String, String)>,
    ) -> AppResult<ListPageResult<Value>> {
        let key = build_list_cache_key("playlists-search", &as_pairs(&extra));
        match plain_query(&extra) {
            Some(q) => {
                sc_search_page(ScSearchArgs {
                    list_cache: &self.list_cache,
                    read: &self.read,
                    ty: SearchType::PlaylistsWithoutAlbums,
                    q,
                    cache_key: &key,
                    ttl: TTL_SEARCH,
                    page,
                    limit,
                })
                .await
            }
            None => {
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
        }
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
            Ok(v) => {
                // Сразу сидим локально, чтобы /me/playlists, assert_owner и
                // немедленный последующий add видели плейлист до /me/playlists
                // reconcile. owner_sc_user_id (bare из upsert) — основа assert_owner.
                if let Some(urn) = v.get("urn").and_then(|u| u.as_str()) {
                    let repo = PlaylistRepository::new(self.pg.clone());
                    let _ = repo.upsert_from_sc(&v).await;
                    let _ = sqlx::query_file!(
                        "queries/playlists/service/insert_owned_playlist.sql",
                        extract_sc_id(sc_user_id),
                        urn,
                    )
                    .execute(&self.pg)
                    .await;
                    // Сидим playlist_tracks только если SC вернул tracks (даже
                    // пустой массив — валидный seed). Если поля нет — оставляем
                    // tracks_synced_at NULL, чтобы get_tracks дотянул из SC.
                    if let Some(tracks) = v.get("tracks").and_then(|t| t.as_array()) {
                        let ids: Vec<String> = tracks
                            .iter()
                            .filter_map(|t| t.get("urn").and_then(|u| u.as_str()))
                            .map(|u| extract_sc_id(u).to_string())
                            .collect();
                        let _ = repo.replace_tracks(urn, &ids).await;
                    }
                }
                Ok(v)
            }
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
            // Sharing-guard для приватных плейлистов. sc_user_id из сессии —
            // URN ("soundcloud:users:NNN"), owner_sc_user_id в БД — голый ID.
            if row.sharing != "public" {
                let me = crate::common::sc_ids::extract_sc_id(sc_user_id);
                let is_owner = row
                    .owner_sc_user_id
                    .as_deref()
                    .map(|u| u == me)
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
                let urn = playlist_urn.to_string();
                tokio::spawn(async move {
                    if let Err(e) = refresh
                        .refresh_playlist(&urn, TokenKind::UserFirst(session_id))
                        .await
                    {
                        tracing::debug!(error = %e, urn = %urn, "playlist refresh failed");
                    }
                });
            }
            return Ok(crate::modules::playlists::project_to_sc_shape(&row, None));
        }
        let fetched = self
            .read
            .playlist_meta(
                TokenKind::UserFirst(session_id),
                extract_sc_id(playlist_urn),
            )
            .await?;
        repo.upsert_from_sc(&fetched).await?;
        Ok(fetched)
    }

    /// Legacy `PUT /playlists/{urn}` с `{ playlist: { tracks: [...] } }`.
    /// Local-first: пишем desired-state в нашу БД, sync в SC — фоном. По
    /// умолчанию MERGE (submitted ∪ current) — устаревшая клиентская вью не
    /// дропает треки; `?replace=true` — честная перестановка из свежей вью.
    pub async fn update(
        &self,
        session_id: Uuid,
        sc_user_id: &str,
        playlist_urn: &str,
        body: &Value,
        replace: bool,
    ) -> AppResult<Value> {
        let me = extract_sc_id(sc_user_id);
        let repo = PlaylistRepository::new(self.pg.clone());
        repo.assert_owner(me, playlist_urn).await?;
        self.ensure_desired_seeded(session_id, playlist_urn, &repo)
            .await?;

        let tracks = body
            .get("playlist")
            .and_then(|p| p.get("tracks"))
            .and_then(|t| t.as_array());
        if let Some(tracks) = tracks {
            let ids: Vec<String> = tracks
                .iter()
                .filter_map(|t| t.get("urn").and_then(|v| v.as_str()))
                .map(|u| extract_sc_id(u).to_string())
                .collect();
            if replace {
                repo.set_order(playlist_urn, ids).await?;
            } else {
                repo.merge_order(playlist_urn, ids).await?;
            }
            self.sync_queue
                .enqueue(sc_user_id, "playlist_sync", playlist_urn, None)
                .await?;
        }
        Ok(json!({ "status": "ok", "targetUrn": playlist_urn }))
    }

    /// Применяет одну дельту membership к desired-state и возвращает свежий
    /// авторитетный список. Owner-check по нашей БД; дельта считается на сервере
    /// против сохранённого desired-state. Sync в SC — фоном через `playlist_sync`.
    pub async fn edit_tracks(
        &self,
        session_id: Uuid,
        sc_user_id: &str,
        playlist_urn: &str,
        edit: TrackEdit,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let me = extract_sc_id(sc_user_id);
        let repo = PlaylistRepository::new(self.pg.clone());
        repo.assert_owner(me, playlist_urn).await?;
        self.ensure_desired_seeded(session_id, playlist_urn, &repo)
            .await?;

        match edit {
            TrackEdit::Add { track_urn } => {
                repo.add_track(playlist_urn, extract_sc_id(&track_urn).to_string())
                    .await?;
            }
            TrackEdit::Remove { track_urn } => {
                repo.remove_track(playlist_urn, extract_sc_id(&track_urn).to_string())
                    .await?;
            }
            TrackEdit::Move {
                track_urn,
                to_index,
            } => {
                repo.move_track(
                    playlist_urn,
                    extract_sc_id(&track_urn).to_string(),
                    to_index.max(0) as usize,
                )
                .await?;
            }
            TrackEdit::SetOrder { track_urns } => {
                let ids = track_urns
                    .iter()
                    .map(|u| extract_sc_id(u).to_string())
                    .collect();
                repo.set_order(playlist_urn, ids).await?;
            }
        }

        self.sync_queue
            .enqueue(sc_user_id, "playlist_sync", playlist_urn, None)
            .await?;
        // Owner всегда видит private-членов своего плейлиста.
        self.project_page(playlist_urn, true, page, limit).await
    }

    /// Гарантирует, что desired-state (`playlist_tracks`) засижен — у дельты
    /// должна быть база. Нет строки → UPSERT меты из SC; tracks_synced_at IS NULL
    /// и нет pending → один синхронный refresh. При pending-intent ничего не
    /// трогаем: desired-state уже наш.
    async fn ensure_desired_seeded(
        &self,
        session_id: Uuid,
        playlist_urn: &str,
        repo: &PlaylistRepository,
    ) -> AppResult<()> {
        let row = repo.find_by_urn(playlist_urn).await?;
        let needs_meta = row.is_none();
        let needs_tracks = match &row {
            None => true,
            Some(r) => r.tracks_synced_at.is_none(),
        };
        if !needs_meta && !needs_tracks {
            return Ok(());
        }
        if repo.has_pending_intent(playlist_urn).await? {
            return Ok(());
        }
        let kind = TokenKind::UserFirst(session_id);
        if needs_meta {
            let fetched = self
                .read
                .playlist_meta(kind, extract_sc_id(playlist_urn))
                .await?;
            repo.upsert_from_sc(&fetched).await?;
        }
        if needs_tracks {
            self.cold_refresh
                .refresh_playlist_tracks(playlist_urn, kind)
                .await?;
        }
        Ok(())
    }

    /// Проекция страницы из `playlist_tracks` (desired-state) в SC-shape.
    async fn project_page(
        &self,
        playlist_urn: &str,
        can_see_private: bool,
        page: i64,
        limit: i64,
    ) -> AppResult<ListPageResult<Value>> {
        let repo = PlaylistRepository::new(self.pg.clone());
        let offset = page.max(0) * limit;
        let ids = repo.page_track_ids(playlist_urn, offset, limit + 1).await?;
        let has_more = ids.len() as i64 > limit;
        let page_ids: Vec<String> = ids.into_iter().take(limit as usize).collect();
        let projected = if can_see_private {
            crate::modules::tracks::project_many(&self.pg, &page_ids).await?
        } else {
            crate::modules::tracks::project_many_public(&self.pg, &page_ids).await?
        };
        let collection: Vec<Value> = projected.into_iter().flatten().collect();
        Ok(ListPageResult {
            collection,
            page,
            page_size: limit,
            has_more,
        })
    }

    /// Смена приватности своего плейлиста. Owner-check по нашей БД, optimistic
    /// апдейт `playlists.sharing`, write-back в SC через sync_queue
    /// (`playlist_sharing` — без destructive-инвалидации строки).
    pub async fn set_sharing(
        &self,
        sc_user_id: &str,
        playlist_urn: &str,
        sharing: &str,
    ) -> AppResult<Value> {
        if sharing != "public" && sharing != "private" {
            return Err(AppError::bad_request(
                "sharing must be 'public' or 'private'",
            ));
        }
        let me = crate::common::sc_ids::extract_sc_id(sc_user_id);
        let owner: Option<Option<String>> =
            sqlx::query_file_scalar!("queries/playlists/service/select_owner.sql", playlist_urn)
                .fetch_optional(&self.pg)
                .await?;
        match owner {
            Some(o) if o.as_deref() == Some(me) => {}
            _ => return Err(AppError::not_found("Playlist not found")),
        }

        sqlx::query_file!(
            "queries/playlists/service/update_sharing.sql",
            playlist_urn,
            sharing
        )
        .execute(&self.pg)
        .await?;
        self.sync_queue
            .enqueue(
                sc_user_id,
                "playlist_sharing",
                playlist_urn,
                Some(&json!({ "sharing": sharing })),
            )
            .await?;
        Ok(json!({ "urn": playlist_urn, "sharing": sharing }))
    }

    /// Оптимистичный delete: убираем строку из user_owned_playlists (UI сразу
    /// перестаёт показывать плейлист в /me/playlists), сносим playlists +
    /// playlist_tracks. SC delete — фоном через worker.
    pub async fn delete(&self, sc_user_id: &str, playlist_urn: &str) -> AppResult<Value> {
        let mut tx = self.pg.begin().await?;
        // owned-mirror ключуется bare; чистим оба варианта на случай старой
        // URN-строки до бэкфилла (удаляем плейлист — сносим все owned-рефы).
        let user_variants = crate::common::sc_ids::user_id_variants(sc_user_id);
        sqlx::query_file!(
            "queries/playlists/service/delete_owned.sql",
            &user_variants,
            playlist_urn
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query_file!(
            "queries/playlists/service/delete_playlist.sql",
            playlist_urn
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query_file!(
            "queries/playlists/service/delete_playlist_tracks.sql",
            playlist_urn
        )
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

        let viewer = crate::common::sc_ids::extract_sc_id(sc_user_id);
        let guard_private = |row: &crate::modules::playlists::PlaylistRow| -> AppResult<()> {
            if row.sharing != "public" && row.owner_sc_user_id.as_deref() != Some(viewer) {
                return Err(AppError::not_found("Playlist not found"));
            }
            Ok(())
        };

        let mut playlist_row = repo.find_by_urn(playlist_urn).await?;
        if let Some(row) = &playlist_row {
            guard_private(row)?;
        }
        let needs_seed = match &playlist_row {
            None => true,
            Some(r) => r.tracks_synced_at.is_none(),
        };

        if needs_seed {
            let kind = TokenKind::UserFirst(session_id);
            // Если плейлиста ещё нет в `playlists` — UPSERT meta перед track-list.
            if playlist_row.is_none() {
                let fetched = self
                    .read
                    .playlist_meta(kind, extract_sc_id(playlist_urn))
                    .await?;
                repo.upsert_from_sc(&fetched).await?;
            }
            self.cold_refresh
                .refresh_playlist_tracks(playlist_urn, kind)
                .await?;
            // Первый заход (row был None): перечитываем мету, иначе can_see_private
            // ниже посчитается по None → owner своего приватного плейлиста увидел
            // бы public-only до второго захода. Свежую мету тоже guard'им.
            if playlist_row.is_none() {
                playlist_row = repo.find_by_urn(playlist_urn).await?;
                if let Some(row) = &playlist_row {
                    guard_private(row)?;
                }
            }
        } else if let Some(row) = &playlist_row {
            // Owner: desired-state — истина, фоновый re-pull НЕ нужен (и затёр бы
            // pending-правку через replace_tracks, хоть тот и гейтит). Non-owner —
            // обычный SWR из SC.
            let is_owner = row.owner_sc_user_id.as_deref() == Some(viewer);
            if !is_owner && self.cold_refresh.is_playlist_stale(row.tracks_synced_at) {
                let refresh = self.cold_refresh.clone();
                let urn = playlist_urn.to_string();
                tokio::spawn(async move {
                    if let Err(e) = refresh
                        .refresh_playlist_tracks(&urn, TokenKind::UserFirst(session_id))
                        .await
                    {
                        tracing::debug!(error = %e, urn = %urn, "playlist tracks refresh failed");
                    }
                });
            }
        }

        // Приватные member-треки видит только их uploader. Приватный плейлист
        // выше уже owner-guarded (sharing != public ⇒ caller — owner), публичный
        // показывает private-членов лишь своему владельцу. Иначе — public-only.
        let can_see_private = playlist_row.as_ref().is_some_and(|r| {
            r.sharing != "public" || r.owner_sc_user_id.as_deref() == Some(viewer)
        });

        self.project_page(playlist_urn, can_see_private, page, limit)
            .await
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

fn as_pairs(v: &[(String, String)]) -> Vec<(&str, String)> {
    v.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
}
