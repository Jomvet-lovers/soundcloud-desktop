use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tokio::sync::{OnceCell, Semaphore};
use tracing::{debug, warn};

use crate::cache::{CacheService, ListPageResult};
use crate::common::sc_ids::extract_sc_id;
use crate::config::ColdCfg;
use crate::error::AppResult;
use crate::modules::auth::try_with_chain;
use crate::modules::indexing::IndexingService;
use crate::modules::playlists::PlaylistRepository;
use crate::modules::tracks::{TrackPriority, TrackRepository};
use crate::modules::users::{project_to_sc_shape as project_user, UserRepository};
use crate::sc::ScClient;

const REFRESH_PAGE_LIMIT: u64 = 200;

/// Grace перед orphan-delete: строку, синканную позже `reconcile_started_at -
/// ORPHAN_GRACE`, не удаляем, даже если её нет в авторитетном SC-снапшоте —
/// у SC-листинга есть лаг распространения, свежесинканный из очереди лайк
/// ещё может не попасть в листинг. Настоящий orphan (юзер снял лайк на вебе)
/// имеет старый synced_at (перестал обновляться) и проходит фильтр.
const ORPHAN_GRACE_SEC: i64 = 300;

/// Per-user коллекция из SC, которую мы зеркалируем в свою БД.
///
/// `mirror_table` хранит "что юзер хочет" — like/follow/own state, с
/// семантикой wanted/progress (см. sync_queue::mirror). `entity_kind` —
/// какие нормализованные сущности дополнительно UPSERT'ить из payload'ов
/// (треки → `tracks` + кик пайплайна; плейлисты → `playlists`;
/// юзеры → `users`).
#[derive(Debug, Clone, Copy)]
pub struct UserCollection {
    /// Путь для `/me/*` — приватный, требует user-токен (отдаёт private items).
    pub sc_path_self: &'static str,
    /// Шаблон для `/users/{}/*` — public-вьюха, доступна любому токену.
    /// `{}` подставляется на sc_user_id владельца.
    pub sc_path_other: &'static str,
    pub lock_kind: &'static str,
    pub mirror_table: &'static str,
    pub mirror_key_col: &'static str,
    pub entity_kind: EntityKind,
    /// Owned-коллекции пишут публичный payload в свой `payload`-столбец
    /// (приватные поля владельца не могут лежать в shared `tracks`/`playlists`).
    pub mirror_payload_col: Option<&'static str>,
    /// true для owned-коллекций — нет inverse-операции (delete симметрична).
    pub has_wanted_state: bool,
    /// Owned-плейлисты: не воскрешать строку из refresh'а, если
    /// `sync_queue` уже содержит pending `playlist_delete` на этот URN.
    pub guard_pending_delete_action: Option<&'static str>,
    /// Priority пайплайна для треков, попадающих в коллекцию (irrelevant
    /// для не-track коллекций).
    pub track_priority: TrackPriority,
}

#[derive(Debug, Clone, Copy)]
pub enum EntityKind {
    Track,
    Playlist,
    User,
}

pub const LIKED_TRACKS: UserCollection = UserCollection {
    sc_path_self: "/me/likes/tracks",
    sc_path_other: "/users/{}/likes/tracks",
    lock_kind: "liked-tracks",
    mirror_table: "user_likes_tracks",
    mirror_key_col: "sc_track_id",
    entity_kind: EntityKind::Track,
    mirror_payload_col: None,
    has_wanted_state: true,
    guard_pending_delete_action: None,
    track_priority: TrackPriority::Like,
};

pub const LIKED_PLAYLISTS: UserCollection = UserCollection {
    sc_path_self: "/me/likes/playlists",
    sc_path_other: "/users/{}/likes/playlists",
    lock_kind: "liked-playlists",
    mirror_table: "user_likes_playlists",
    mirror_key_col: "playlist_urn",
    entity_kind: EntityKind::Playlist,
    mirror_payload_col: None,
    has_wanted_state: true,
    guard_pending_delete_action: None,
    track_priority: TrackPriority::Playlist,
};

pub const FOLLOWINGS: UserCollection = UserCollection {
    sc_path_self: "/me/followings",
    sc_path_other: "/users/{}/followings",
    lock_kind: "followings",
    mirror_table: "user_followings",
    mirror_key_col: "target_user_urn",
    entity_kind: EntityKind::User,
    mirror_payload_col: None,
    has_wanted_state: true,
    guard_pending_delete_action: None,
    track_priority: TrackPriority::Discovery,
};

pub const OWNED_PLAYLISTS: UserCollection = UserCollection {
    sc_path_self: "/me/playlists",
    sc_path_other: "/users/{}/playlists",
    lock_kind: "owned-playlists",
    mirror_table: "user_owned_playlists",
    mirror_key_col: "playlist_urn",
    entity_kind: EntityKind::Playlist,
    mirror_payload_col: None,
    has_wanted_state: false,
    guard_pending_delete_action: Some("playlist_delete"),
    track_priority: TrackPriority::Playlist,
};

pub const OWNED_TRACKS: UserCollection = UserCollection {
    sc_path_self: "/me/tracks",
    sc_path_other: "/users/{}/tracks",
    lock_kind: "owned-tracks",
    mirror_table: "user_owned_tracks",
    mirror_key_col: "sc_track_id",
    entity_kind: EntityKind::Track,
    mirror_payload_col: None,
    has_wanted_state: false,
    guard_pending_delete_action: None,
    track_priority: TrackPriority::Like,
};

fn resolve_sc_path(coll: &UserCollection, sc_user_id: &str, viewer_is_owner: bool) -> String {
    if viewer_is_owner {
        coll.sc_path_self.to_string()
    } else {
        coll.sc_path_other.replace("{}", sc_user_id)
    }
}

/// Сервис фоновых refresh-задач cold-storage.
///
/// * SC pagination — следуем `next_href` URL целиком (а не реконструируем
///   query из извлечённого cursor/offset). Этим лечится зацикливание
///   `/playlists/{urn}/tracks` на первых 200 треках (SC ожидает `offset=`,
///   мы клали `cursor=`).
/// * Lock'и через Redis SETNX — параллельные refresh'и одного ресурса
///   тихо отваливаются вторым.
/// * Bounded concurrency — `Semaphore` ограничивает живые SC-fetch'ы,
///   чтобы пики reads не выжигали public-token пул.
/// * На каждый трек из коллекции → [`IndexingService::ingest_track_from_sc`]
///   с приоритетом коллекции; на каждого юзера → `users` UPSERT;
///   на каждый плейлист → `playlists` UPSERT.
pub struct ColdRefreshService {
    sc: ScClient,
    pg: PgPool,
    cache: Arc<CacheService>,
    cfg: ColdCfg,
    sem: Arc<Semaphore>,
    tracks: TrackRepository,
    users: UserRepository,
    playlists: PlaylistRepository,
    indexing: OnceCell<Arc<IndexingService>>,
}

impl ColdRefreshService {
    pub fn new(sc: ScClient, pg: PgPool, cache: Arc<CacheService>, cfg: ColdCfg) -> Arc<Self> {
        let sem = Arc::new(Semaphore::new(cfg.refresh_concurrency));
        let tracks = TrackRepository::new(pg.clone());
        let users = UserRepository::new(pg.clone());
        let playlists = PlaylistRepository::new(pg.clone());
        Arc::new(Self {
            sc,
            pg,
            cache,
            cfg,
            sem,
            tracks,
            users,
            playlists,
            indexing: OnceCell::new(),
        })
    }

    /// Поздняя инъекция IndexingService — в main.rs он создаётся позже из-за
    /// transcode/lyrics-зависимостей. Без него track-ingestion не кикает
    /// пайплайн (но UPSERT всё равно работает — это safe degrade).
    pub fn install_indexing(&self, indexing: Arc<IndexingService>) {
        let _ = self.indexing.set(indexing);
    }

    /// Доступ к привязанному IndexingService для callers, которым нужно
    /// прокинуть свежий SC payload через ingest-pipeline (например, tracks/
    /// service::get_by_id при cache-miss).
    pub fn indexing_for_ingest(&self) -> Option<&Arc<IndexingService>> {
        self.indexing.get()
    }

    pub fn is_track_stale(&self, synced_at: Option<DateTime<Utc>>) -> bool {
        is_stale(synced_at, self.cfg.track_ttl_sec)
    }

    pub fn is_user_stale(&self, synced_at: Option<DateTime<Utc>>) -> bool {
        is_stale(synced_at, self.cfg.user_ttl_sec)
    }

    pub fn is_playlist_stale(&self, synced_at: Option<DateTime<Utc>>) -> bool {
        is_stale(synced_at, self.cfg.playlist_ttl_sec)
    }

    fn ttl_for(&self, coll: &UserCollection) -> u64 {
        match coll.lock_kind {
            k if k == LIKED_TRACKS.lock_kind => self.cfg.liked_tracks_ttl_sec,
            k if k == LIKED_PLAYLISTS.lock_kind => self.cfg.liked_playlists_ttl_sec,
            k if k == FOLLOWINGS.lock_kind => self.cfg.followings_ttl_sec,
            _ => self.cfg.owned_ttl_sec,
        }
    }

    /// Гарантирует, что mirror юзера для коллекции достаточно свежий.
    /// Пустое зеркало — синхронный seed. Stale — фоновый refresh
    /// (первый клиент после TTL заплатит, остальные читают текущий снапшот).
    /// Свежее — no-op.
    ///
    /// `viewer_is_owner` определяет path: `true` → `/me/*` (видит private),
    /// `false` → `/users/{id}/*` (public-вьюха). Одна и та же mirror-таблица
    /// для обоих случаев — `target_sc_user_id` индексирует строки.
    pub async fn ensure_collection(
        self: &Arc<Self>,
        coll: UserCollection,
        sc_user_id: &str,
        viewer_is_owner: bool,
        chain: &[String],
        extra_params: &[(String, String)],
    ) -> AppResult<()> {
        let max_synced: Option<DateTime<Utc>> = sqlx::query_scalar(&format!(
            "SELECT MAX(synced_at) FROM {} WHERE user_id = ANY($1)",
            coll.mirror_table
        ))
        .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
        .fetch_one(&self.pg)
        .await?;

        if max_synced.is_none() {
            self.refresh_collection(coll, sc_user_id, viewer_is_owner, chain, extra_params)
                .await?;
            return Ok(());
        }
        if !is_stale(max_synced, self.ttl_for(&coll)) {
            return Ok(());
        }

        let me = Arc::clone(self);
        let user = sc_user_id.to_string();
        let chain = chain.to_vec();
        let extra = extra_params.to_vec();
        tokio::spawn(async move {
            if let Err(e) = me
                .refresh_collection(coll, &user, viewer_is_owner, &chain, &extra)
                .await
            {
                debug!(error = %e, user = %user, kind = coll.lock_kind, "background refresh failed");
            }
        });
        Ok(())
    }

    /// Полный refresh per-user коллекции из SC. Тянет все страницы
    /// (через `next_href`), UPSERT'ит сущности и mirror, удаляет orphan'ы.
    pub async fn refresh_collection(
        &self,
        coll: UserCollection,
        sc_user_id: &str,
        viewer_is_owner: bool,
        chain: &[String],
        extra_params: &[(String, String)],
    ) -> AppResult<()> {
        let key = format!("refresh:{}:{sc_user_id}", coll.lock_kind);
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let path = resolve_sc_path(&coll, sc_user_id, viewer_is_owner);
        // Старт фиксируем ДО фетча: orphan-delete не трогает строки, синканные
        // после этого момента (см. delete_orphans grace-window).
        let reconcile_started_at = Utc::now();
        let (items, complete) = self.fetch_all_pages(&path, chain, extra_params).await?;

        // SC отдаёт новые сверху; разворачиваем под наш ORDER BY created_at DESC.
        let ordered: Vec<(String, &Value)> = items
            .iter()
            .rev()
            .filter_map(|item| {
                let urn = item.get("urn").and_then(|v| v.as_str()).unwrap_or("");
                if urn.is_empty() {
                    return None;
                }
                let key_value = match coll.entity_kind {
                    EntityKind::Track => extract_sc_id(urn).to_string(),
                    EntityKind::Playlist | EntityKind::User => urn.to_string(),
                };
                Some((key_value, item))
            })
            .collect();

        // Owned-плейлисты с pending локальной правкой (desired_rev > synced_rev)
        // НЕ перезатираем из SC — наш track_count/мета побеждают до синка.
        let pending_skip: std::collections::HashSet<String> =
            if coll.lock_kind == OWNED_PLAYLISTS.lock_kind {
                self.playlists
                    .pending_owned_urns(extract_sc_id(sc_user_id))
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            } else {
                std::collections::HashSet::new()
            };

        // Сначала — все entity UPSERT'ы (ingest_track кикает пайплайн).
        // Параллелизм отсутствует намеренно: на коллекции в 10к треков
        // создавать 10к тасок и одновременно дёргать transcode/enrich — это
        // pile-up. Sequential UPSERT держит pressure под semaphore'ом.
        for (key_value, item) in &ordered {
            if pending_skip.contains(key_value) {
                continue;
            }
            if let Err(e) = self.ingest_entity(coll, item).await {
                warn!(error = %e, kind = coll.lock_kind, "entity ingest failed");
            }
        }

        // Затем — bulk UPSERT mirror-таблицы.
        let mirror_keys: Vec<String> = ordered.iter().map(|(k, _)| k.clone()).collect();
        let mirror_payloads: Vec<Value> = if coll.mirror_payload_col.is_some() {
            ordered.iter().map(|(_, item)| (*item).clone()).collect()
        } else {
            Vec::new()
        };

        if !mirror_keys.is_empty() {
            let mut tx = self.pg.begin().await?;
            // Канон ключа mirror — bare (закрываем URN/bare split на write-path).
            batch_upsert_mirror(
                &mut tx,
                &coll,
                extract_sc_id(sc_user_id),
                &mirror_keys,
                &mirror_payloads,
            )
            .await?;
            tx.commit().await?;
        }

        // Additive+guarded. Orphan-delete ТОЛЬКО когда: пагинация дошла до конца
        // авторитетно (complete), это owner-вью /me/* (публичный /users/{id}/*
        // снапшот не содержит private — не может авторизовать удаление из общего
        // mirror), и снапшот непустой. Иначе обрезанный/лагающий ответ молча
        // затёр бы свежий лайк — это и был баг «лайк пропал».
        if complete && viewer_is_owner && !items.is_empty() {
            delete_orphans(
                &self.pg,
                &coll,
                extract_sc_id(sc_user_id),
                &mirror_keys,
                reconcile_started_at,
            )
            .await?;
        }
        Ok(())
    }

    async fn ingest_entity(&self, coll: UserCollection, item: &Value) -> AppResult<()> {
        match coll.entity_kind {
            EntityKind::Track => {
                if let Some(indexing) = self.indexing.get() {
                    indexing
                        .ingest_track_from_sc(item, coll.track_priority)
                        .await?;
                } else {
                    // Indexing ещё не подключён (раннее spawn'ение) —
                    // делаем хотя бы UPSERT в tracks без kick'а пайплайна.
                    if let Some(fields) =
                        crate::modules::tracks::normalize::ScTrackFields::from_sc(item)
                    {
                        self.tracks
                            .upsert_from_sc(&fields, coll.track_priority, coll.track_priority)
                            .await?;
                    }
                }
            }
            EntityKind::Playlist => {
                self.playlists.upsert_from_sc(item).await?;
            }
            EntityKind::User => {
                self.users.upsert_from_sc(item).await?;
            }
        }
        Ok(())
    }

    pub async fn refresh_track(
        self: &Arc<Self>,
        track_urn: &str,
        chain: &[String],
    ) -> AppResult<()> {
        let key = format!("refresh:track:{track_urn}");
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let fetched: Value = try_with_chain(chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/tracks/{track_urn}");
            async move { sc.api_get_value(&path, &tok, None).await }
        })
        .await?;
        if let Some(indexing) = self.indexing.get() {
            indexing
                .ingest_track_from_sc(&fetched, TrackPriority::Discovery)
                .await?;
        } else if let Some(fields) =
            crate::modules::tracks::normalize::ScTrackFields::from_sc(&fetched)
        {
            self.tracks
                .upsert_from_sc(&fields, TrackPriority::Discovery, TrackPriority::Discovery)
                .await?;
        }
        debug!(urn = %track_urn, "track refreshed");
        Ok(())
    }

    pub async fn refresh_user(&self, user_urn: &str, chain: &[String]) -> AppResult<()> {
        let key = format!("refresh:user:{user_urn}");
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let fetched: Value = try_with_chain(chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/users/{user_urn}");
            async move { sc.api_get_value(&path, &tok, None).await }
        })
        .await?;
        self.users.upsert_from_sc(&fetched).await?;
        debug!(urn = %user_urn, "user refreshed");
        Ok(())
    }

    pub async fn refresh_playlist(&self, playlist_urn: &str, chain: &[String]) -> AppResult<()> {
        let key = format!("refresh:playlist:{playlist_urn}");
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let fetched: Value = try_with_chain(chain, |tok| {
            let sc = self.sc.clone();
            let path = format!("/playlists/{playlist_urn}");
            async move { sc.api_get_value(&path, &tok, None).await }
        })
        .await?;
        self.playlists.upsert_from_sc(&fetched).await?;
        debug!(urn = %playlist_urn, "playlist refreshed");
        Ok(())
    }

    /// Полный refresh tracks-list плейлиста (для отдельной SWR-цепочки на
    /// /playlists/{urn}/tracks). Идёт по next_href, ingest'ит каждый трек,
    /// атомарно подменяет playlist_tracks через replace_tracks.
    pub async fn refresh_playlist_tracks(
        self: &Arc<Self>,
        playlist_urn: &str,
        chain: &[String],
    ) -> AppResult<()> {
        let key = format!("refresh:playlist-tracks:{playlist_urn}");
        let Some(_lock) = self.try_lock(&key).await? else {
            return Ok(());
        };
        let _permit = self.sem.acquire().await.ok();
        let (items, complete) = self
            .fetch_all_pages(&format!("/playlists/{playlist_urn}/tracks"), chain, &[])
            .await?;

        let mut ordered_ids: Vec<String> = Vec::with_capacity(items.len());
        for item in &items {
            let Some(urn) = item.get("urn").and_then(|v| v.as_str()) else {
                continue;
            };
            ordered_ids.push(extract_sc_id(urn).to_string());
            if let Some(indexing) = self.indexing.get() {
                if let Err(e) = indexing
                    .ingest_track_from_sc(item, TrackPriority::Playlist)
                    .await
                {
                    debug!(error = %e, "playlist track ingest failed");
                }
            }
        }
        // На обрезанном снапшоте не даём replace_tracks УКОРОТИТЬ уже собранный
        // плейлист (truncation затёрла бы треки). Рост/равенство — ок (заодно
        // покрывает ровно-200-трековый плейлист, который эвристика complete
        // помечает false). replace_tracks дополнительно гейтит pending-intent.
        if !complete {
            let current: i64 = sqlx::query_file_scalar!(
                "queries/cold_refresh/service/count_playlist_tracks.sql",
                playlist_urn
            )
            .fetch_one(&self.pg)
            .await?;
            if (ordered_ids.len() as i64) < current {
                debug!(
                    urn = %playlist_urn,
                    got = ordered_ids.len(),
                    current,
                    "incomplete playlist snapshot; skip replace to avoid shrink"
                );
                return Ok(());
            }
        }
        self.playlists
            .replace_tracks(playlist_urn, &ordered_ids)
            .await?;
        Ok(())
    }

    /// Идёт по SC pagination через `next_href` URL целиком (а не пересобирая
    /// query из cursor/offset — этим лечится баг с зацикливанием
    /// `/playlists/{urn}/tracks` на первых 200 треках). На первой странице
    /// формируем params сами; дальше SC отдаёт абсолютный URL, на который
    /// идём как есть.
    /// Возвращает `(items, complete)`. `complete=false` сигналит, что пагинация
    /// оборвалась неавторитетно (обрезка/зацикливание) и снапшот НЕ полный —
    /// caller не имеет права на его основе удалять локальные строки. Жёсткая
    /// ошибка по-прежнему прерывает через `?` (до delete-логики не доходит).
    async fn fetch_all_pages(
        &self,
        path: &str,
        chain: &[String],
        extra_params: &[(String, String)],
    ) -> AppResult<(Vec<Value>, bool)> {
        let mut acc: Vec<Value> = Vec::new();
        let mut next: Option<String> = None;
        let complete = loop {
            let resp: Value = match &next {
                None => {
                    let mut params: Vec<(String, String)> =
                        Vec::with_capacity(2 + extra_params.len());
                    params.extend(extra_params.iter().cloned());
                    params.push(("limit".into(), REFRESH_PAGE_LIMIT.to_string()));
                    params.push(("linked_partitioning".into(), "true".into()));
                    try_with_chain(chain, |tok| {
                        let sc = self.sc.clone();
                        let path = path.to_string();
                        let params = params.clone();
                        async move { sc.api_get_value(&path, &tok, Some(&params)).await }
                    })
                    .await?
                }
                Some(href) => {
                    try_with_chain(chain, |tok| {
                        let sc = self.sc.clone();
                        let href = href.clone();
                        async move { sc.api_get_absolute_value(&href, &tok).await }
                    })
                    .await?
                }
            };
            let items: Vec<Value> = resp
                .get("collection")
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
            if items.is_empty() {
                // Пустая ПЕРВАЯ страница — легитимный «нет элементов» (complete).
                // Пустая промежуточная (идём по next_href) — обрыв (incomplete).
                break next.is_none();
            }
            let full_page = items.len() as u64 == REFRESH_PAGE_LIMIT;
            acc.extend(items);
            match resp.get("next_href").and_then(|v| v.as_str()) {
                // Нет/пустой курсор после ПОЛНОЙ страницы — SC обрезал пагинацию,
                // хотя элементы ещё есть. После короткой — естественный конец.
                None | Some("") => break !full_page,
                // Курсор не сдвинулся — зацикливание, бьёмся об тот же href.
                Some(href) if Some(href) == next.as_deref() => break false,
                Some(href) => next = Some(href.to_string()),
            }
        };
        Ok((acc, complete))
    }

    async fn try_lock(&self, key: &str) -> AppResult<Option<()>> {
        let acquired = self
            .cache
            .try_acquire_lock(key, self.cfg.refresh_lock_ttl_sec)
            .await?;
        Ok(if acquired { Some(()) } else { None })
    }
}

fn is_stale(synced_at: Option<DateTime<Utc>>, ttl_sec: u64) -> bool {
    match synced_at {
        None => true,
        Some(t) => {
            let age = Utc::now().signed_duration_since(t).num_seconds();
            age < 0 || age as u64 > ttl_sec
        }
    }
}

async fn batch_upsert_mirror(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    coll: &UserCollection,
    sc_user_id: &str,
    keys: &[String],
    payloads: &[Value],
) -> AppResult<()> {
    let key_col = coll.mirror_key_col;
    let table = coll.mirror_table;

    // Используем clock_timestamp() (volatile per-row) для created_at, чтобы
    // refresh-батчи получали разные ts и ORDER BY (created_at DESC, key DESC)
    // сохранял SC-порядок. ON CONFLICT updates НЕ переписывают created_at.
    let (select_cols, update_set) = if let Some(p) = coll.mirror_payload_col {
        (
            "$1, t.k, t.p, false, now(), clock_timestamp()".to_string(),
            format!("{p} = EXCLUDED.{p}, synced_at = now()"),
        )
    } else if coll.has_wanted_state {
        (
            "$1, t.k, true, false, now(), clock_timestamp()".to_string(),
            // SC показывает этот ключ как активный лайк. Сбрасываем progress в
            // false ТОЛЬКО для строк, чьё локальное намерение всё ещё «лайкнуто»
            // (synced/pending like). Pending-unlike (wanted_state=false) НЕ
            // воскрешаем — только synced_at, чтобы unlike доехал, а delete_orphans
            // (только полный снапшот) был единственным, кто его уберёт.
            format!(
                "synced_at = now(), \
                 progress = CASE WHEN {table}.wanted_state = true \
                                 THEN false ELSE {table}.progress END"
            ),
        )
    } else {
        (
            "$1, t.k, false, now(), clock_timestamp()".to_string(),
            // Owned не имеет pending-unlike; SC показал строку — создание
            // подтверждено, чистим progress.
            "synced_at = now(), progress = false".to_string(),
        )
    };

    let insert_cols = if let Some(col) = coll.mirror_payload_col {
        format!("user_id, {key_col}, {col}, progress, synced_at, created_at")
    } else if coll.has_wanted_state {
        format!("user_id, {key_col}, wanted_state, progress, synced_at, created_at")
    } else {
        format!("user_id, {key_col}, progress, synced_at, created_at")
    };

    let from_clause = if coll.mirror_payload_col.is_some() {
        "FROM UNNEST($2::text[], $3::jsonb[]) AS t(k, p)"
    } else {
        "FROM UNNEST($2::text[]) AS t(k)"
    };

    let guard_clause = if let Some(g) = coll.guard_pending_delete_action {
        format!(
            "WHERE NOT EXISTS ( \
                 SELECT 1 FROM sync_queue \
                 WHERE user_id = $1 AND action_type = '{g}' AND target_urn = t.k \
             )"
        )
    } else {
        String::new()
    };

    let sql = format!(
        "INSERT INTO {table} ({insert_cols}) \
         SELECT {select_cols} {from_clause} {guard_clause} \
         ON CONFLICT (user_id, {key_col}) DO UPDATE SET {update_set}"
    );

    let q = sqlx::query(&sql).bind(sc_user_id).bind(keys);
    if coll.mirror_payload_col.is_some() {
        q.bind(payloads).execute(&mut **tx).await?;
    } else {
        q.execute(&mut **tx).await?;
    }
    Ok(())
}

async fn delete_orphans(
    pg: &PgPool,
    coll: &UserCollection,
    sc_user_id: &str,
    seen: &[String],
    started: DateTime<Utc>,
) -> AppResult<()> {
    let cutoff = started - chrono::Duration::seconds(ORPHAN_GRACE_SEC);
    let wanted_filter = if coll.has_wanted_state {
        "AND wanted_state = true"
    } else {
        ""
    };
    // progress=false — никогда не трогаем pending локальную запись (намерение).
    // synced_at < cutoff — подтверждённую достаточно давно (SC успел распространить);
    // свежесинканную (в пределах grace) оставляем. Вызывается только на полном
    // owner-снапшоте (см. refresh_collection).
    let sql = format!(
        "DELETE FROM {table} \
         WHERE user_id = $1 \
           {wanted_filter} \
           AND progress = false \
           AND synced_at IS NOT NULL \
           AND synced_at < $3 \
           AND NOT ({key_col} = ANY($2))",
        table = coll.mirror_table,
        key_col = coll.mirror_key_col,
    );
    sqlx::query(&sql)
        .bind(sc_user_id)
        .bind(seen)
        .bind(cutoff)
        .execute(pg)
        .await?;
    Ok(())
}

/// Чтение страницы из mirror-таблицы юзера. JOIN на нормализованную сущность
/// (`tracks`/`users`/`playlists`) + проекция в SC-shape JSON.
///
/// Mirror-таблица шарится между `/me/*` (владелец, видит private) и
/// `/users/{id}/*` (чужой профиль): строки индексируются `target_sc_user_id`,
/// сидятся owner-токеном с private-контентом. Поэтому `public_only` ОБЯЗАН
/// быть `true` для чужого вьювера — иначе приватные строки владельца утекают.
pub async fn read_collection_page(
    pg: &PgPool,
    coll: &UserCollection,
    sc_user_id: &str,
    page: i64,
    limit: i64,
    public_only: bool,
) -> AppResult<ListPageResult<Value>> {
    let offset = page.max(0) * limit;
    let table = coll.mirror_table;
    let key_col = coll.mirror_key_col;
    let wanted_filter = if coll.has_wanted_state {
        "AND m.wanted_state = true"
    } else {
        ""
    };

    // Берём ключи постранично, дальше bulk-проекция через shared таблицы.
    // user_id = ANY(variants) + GROUP BY: до бэкфилла 0042 строки могут жить и
    // под URN, и под bare — видим объединение и схлопываем дубль по ключу
    // (берём самый свежий created_at для порядка).
    let key_sql = format!(
        "SELECT m.{key_col} FROM {table} m \
         WHERE m.user_id = ANY($1) {wanted_filter} \
         GROUP BY m.{key_col} \
         ORDER BY MAX(m.created_at) DESC, m.{key_col} DESC \
         LIMIT $2 OFFSET $3"
    );
    let keys: Vec<(String,)> = sqlx::query_as(&key_sql)
        .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
        .bind(limit + 1)
        .bind(offset)
        .fetch_all(pg)
        .await?;
    let has_more = keys.len() as i64 > limit;
    let page_keys: Vec<String> = keys
        .into_iter()
        .take(limit as usize)
        .map(|(k,)| k)
        .collect();

    let collection: Vec<Value> = match coll.entity_kind {
        EntityKind::Track => {
            let projected = if public_only {
                crate::modules::tracks::project_many_public(pg, &page_keys).await?
            } else {
                crate::modules::tracks::project_many(pg, &page_keys).await?
            };
            projected.into_iter().flatten().collect()
        }
        EntityKind::User => {
            let rows: Vec<crate::modules::users::UserRow> = sqlx::query_file_as!(
                crate::modules::users::UserRow,
                "queries/cold_refresh/service/users_by_urns.sql",
                &page_keys
            )
            .fetch_all(pg)
            .await?;
            let map: std::collections::HashMap<String, crate::modules::users::UserRow> =
                rows.into_iter().map(|u| (u.urn.clone(), u)).collect();
            page_keys
                .iter()
                .filter_map(|urn| map.get(urn).map(project_user))
                .collect()
        }
        EntityKind::Playlist => {
            // public_only фильтрует приватные плейлисты владельца для чужого вьювера.
            let rows: Vec<crate::modules::playlists::PlaylistRow> = if public_only {
                sqlx::query_file_as!(
                    crate::modules::playlists::PlaylistRow,
                    "queries/cold_refresh/service/playlists_by_urns_public.sql",
                    &page_keys
                )
                .fetch_all(pg)
                .await?
            } else {
                sqlx::query_file_as!(
                    crate::modules::playlists::PlaylistRow,
                    "queries/cold_refresh/service/playlists_by_urns.sql",
                    &page_keys
                )
                .fetch_all(pg)
                .await?
            };
            let map: std::collections::HashMap<String, crate::modules::playlists::PlaylistRow> =
                rows.into_iter().map(|p| (p.urn.clone(), p)).collect();
            page_keys
                .iter()
                .filter_map(|urn| map.get(urn))
                .map(|p| crate::modules::playlists::project_to_sc_shape(p, None))
                .collect()
        }
    };

    Ok(ListPageResult {
        collection,
        page,
        page_size: limit,
        has_more,
    })
}
